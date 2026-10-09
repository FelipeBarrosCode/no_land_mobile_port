// AVAudioEngine equivalent of Moonlight iOS's bounded Opus playback path.
#include "noland_audio_renderer.h"

#import <AVFoundation/AVFoundation.h>
#import <AudioToolbox/AudioToolbox.h>

#include <Limelight.h>
#include <opus_multistream.h>

#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <dispatch/dispatch.h>

typedef struct nl_audio_ios_context {
  AVAudioEngine* engine;
  AVAudioPlayerNode* player;
  AVAudioFormat* format;
  OpusMSDecoder* decoder;
  float* decode_buffer;
  int sample_rate;
  int channel_count;
  int samples_per_frame;
  uint32_t target_buffer_ms;
  uint32_t maximum_buffer_ms;
  uint32_t frame_duration_ms;
  bool engine_started;
  __strong dispatch_semaphore_t queue_slots;
  __strong NSMutableArray<id>* observers;
} nl_audio_ios_context_t;

int nl_audio_renderer_init(nl_audio_renderer_t* renderer,
                           int audio_configuration,
                           const POPUS_MULTISTREAM_CONFIGURATION opus_config,
                           int ar_flags) {
  (void)audio_configuration;
  (void)ar_flags;

  if (renderer == NULL || opus_config == NULL) return -1;
  uint32_t target_buffer_ms = renderer->target_buffer_ms;
  uint32_t maximum_buffer_ms = renderer->maximum_buffer_ms;
  nl_audio_renderer_cleanup(renderer);
  renderer->target_buffer_ms = target_buffer_ms;
  renderer->maximum_buffer_ms = maximum_buffer_ms;

  nl_audio_ios_context_t* ctx = calloc(1, sizeof(*ctx));
  if (ctx == NULL) return -1;

  ctx->sample_rate = opus_config->sampleRate;
  ctx->channel_count = opus_config->channelCount;
  ctx->samples_per_frame = opus_config->samplesPerFrame;
  ctx->target_buffer_ms = renderer->target_buffer_ms > 0 ? renderer->target_buffer_ms : 20;
  ctx->maximum_buffer_ms = renderer->maximum_buffer_ms > 0 ? renderer->maximum_buffer_ms : 80;
  if (ctx->maximum_buffer_ms < ctx->target_buffer_ms)
    ctx->maximum_buffer_ms = ctx->target_buffer_ms;

  ctx->frame_duration_ms = opus_config->sampleRate > 0
    ? (uint32_t)(opus_config->samplesPerFrame / (opus_config->sampleRate / 1000))
    : 5;
  if (ctx->frame_duration_ms == 0) ctx->frame_duration_ms = 5;
  uint32_t maximum_frames = MAX(2U, ctx->maximum_buffer_ms / ctx->frame_duration_ms);
  ctx->queue_slots = dispatch_semaphore_create(maximum_frames);

  int error = 0;
  ctx->decoder = opus_multistream_decoder_create(
    opus_config->sampleRate, opus_config->channelCount,
    opus_config->streams, opus_config->coupledStreams,
    opus_config->mapping, &error);
  if (ctx->decoder == NULL || error != OPUS_OK) {
    fprintf(stderr, "[noland-audio] Opus decoder create failed: %s\n", opus_strerror(error));
    free(ctx);
    return -1;
  }

  ctx->decode_buffer = calloc((size_t)opus_config->samplesPerFrame * opus_config->channelCount, sizeof(float));
  if (ctx->decode_buffer == NULL) {
    opus_multistream_decoder_destroy(ctx->decoder);
    free(ctx);
    return -1;
  }

  AVAudioSession* session = [AVAudioSession sharedInstance];
  NSError* sessionError = nil;
  if (![session setCategory:AVAudioSessionCategoryPlayback
                withOptions:AVAudioSessionCategoryOptionMixWithOthers
                      error:&sessionError] ||
      ![session setActive:YES error:&sessionError]) {
    fprintf(stderr, "[noland-audio] AVAudioSession setup failed: %s\n",
            sessionError.localizedDescription.UTF8String);
    opus_multistream_decoder_destroy(ctx->decoder);
    free(ctx->decode_buffer);
    free(ctx);
    return -1;
  }

  // Setup AVAudioEngine after the playback route is active.
  ctx->engine = [[AVAudioEngine alloc] init];
  ctx->player = [[AVAudioPlayerNode alloc] init];
  [ctx->engine attachNode:ctx->player];

  AVAudioChannelCount channels = (AVAudioChannelCount)opus_config->channelCount;
  double sampleRate = (double)opus_config->sampleRate;

  ctx->format = [[AVAudioFormat alloc] initStandardFormatWithSampleRate:sampleRate channels:channels];
  [ctx->engine connect:ctx->player to:ctx->engine.mainMixerNode format:ctx->format];

  NSError* nsError = nil;
  if (![ctx->engine startAndReturnError:&nsError]) {
    fprintf(stderr, "[noland-audio] AVAudioEngine start failed: %s\n", nsError.localizedDescription.UTF8String);
    opus_multistream_decoder_destroy(ctx->decoder);
    free(ctx->decode_buffer);
    ctx->format = nil;
    ctx->player = nil;
    ctx->engine = nil;
    free(ctx);
    return -1;
  }
  ctx->engine_started = true;

  ctx->observers = [NSMutableArray array];
  __block nl_audio_ios_context_t* block_ctx = ctx;
  id interruption = [NSNotificationCenter.defaultCenter
      addObserverForName:AVAudioSessionInterruptionNotification
      object:session
      queue:NSOperationQueue.mainQueue
      usingBlock:^(NSNotification* note) {
    NSNumber* type = note.userInfo[AVAudioSessionInterruptionTypeKey];
    if (type.unsignedIntegerValue == AVAudioSessionInterruptionTypeBegan) {
      [block_ctx->player pause];
    } else {
      [session setActive:YES error:nil];
      if (!block_ctx->engine.isRunning) [block_ctx->engine startAndReturnError:nil];
      if (!block_ctx->player.isPlaying) [block_ctx->player play];
    }
  }];
  [ctx->observers addObject:interruption];
  id route = [NSNotificationCenter.defaultCenter
      addObserverForName:AVAudioSessionRouteChangeNotification
      object:session
      queue:NSOperationQueue.mainQueue
      usingBlock:^(NSNotification* note) {
    (void)note;
    if (!block_ctx->engine.isRunning) [block_ctx->engine startAndReturnError:nil];
    if (!block_ctx->player.isPlaying) [block_ctx->player play];
  }];
  [ctx->observers addObject:route];

  fprintf(stderr, "[noland-audio] initialized: %dch %dHz %d samples/frame\n",
    opus_config->channelCount, opus_config->sampleRate, opus_config->samplesPerFrame);

  renderer->platform_context = ctx;
  return 0;
}

void nl_audio_renderer_start(nl_audio_renderer_t* renderer) {
  if (renderer == NULL || renderer->platform_context == NULL) return;
  nl_audio_ios_context_t* ctx = (nl_audio_ios_context_t*)renderer->platform_context;
  if (!ctx->player.isPlaying) [ctx->player play];
  fprintf(stderr, "[noland-audio] playback started\n");
}

void nl_audio_renderer_stop(nl_audio_renderer_t* renderer) {
  if (renderer == NULL || renderer->platform_context == NULL) return;
  nl_audio_ios_context_t* ctx = (nl_audio_ios_context_t*)renderer->platform_context;
  if (ctx->player.isPlaying) [ctx->player stop];
}

void nl_audio_renderer_cleanup(nl_audio_renderer_t* renderer) {
  if (renderer == NULL) return;
  nl_audio_ios_context_t* ctx = (nl_audio_ios_context_t*)renderer->platform_context;
  if (ctx == NULL) { memset(renderer, 0, sizeof(*renderer)); return; }

  if (ctx->player) [ctx->player stop];
  if (ctx->engine && ctx->engine_started) [ctx->engine stop];
  for (id observer in ctx->observers) [NSNotificationCenter.defaultCenter removeObserver:observer];
  [ctx->observers removeAllObjects];
  ctx->observers = nil;
  if (ctx->decoder) { opus_multistream_decoder_destroy(ctx->decoder); ctx->decoder = NULL; }
  if (ctx->decode_buffer) { free(ctx->decode_buffer); ctx->decode_buffer = NULL; }
  ctx->format = nil;
  ctx->player = nil;
  ctx->engine = nil;
  ctx->queue_slots = nil;
  free(ctx);
  renderer->platform_context = NULL;
  memset(renderer, 0, sizeof(*renderer));
}

void nl_audio_renderer_decode_and_play_sample(nl_audio_renderer_t* renderer,
                                              char* sample_data,
                                              int sample_length) {
  if (renderer == NULL || renderer->platform_context == NULL || sample_length < 0) return;
  nl_audio_ios_context_t* ctx = (nl_audio_ios_context_t*)renderer->platform_context;
  if (ctx->decoder == NULL || ctx->decode_buffer == NULL || ctx->player == nil) return;

  if (sample_length > 0 && LiGetPendingAudioDuration() > 30) return;

  const unsigned char* opus_data = sample_data ? (const unsigned char*)sample_data : NULL;
  int decoded = opus_multistream_decode_float(
    ctx->decoder, opus_data, sample_length,
    ctx->decode_buffer, ctx->samples_per_frame, 0);

  if (decoded <= 0) {
    fprintf(stderr, "[noland-audio] decode failed: %s\n", opus_strerror(decoded));
    return;
  }

  if (dispatch_semaphore_wait(ctx->queue_slots, DISPATCH_TIME_NOW) != 0) {
    // Bound client-side audio latency rather than allowing AVAudioEngine's
    // scheduled-buffer queue to grow indefinitely after a stall.
    return;
  }

  AVAudioFrameCount frame_count = (AVAudioFrameCount)decoded;
  AVAudioPCMBuffer* buffer = [[AVAudioPCMBuffer alloc] initWithPCMFormat:ctx->format
                                                           frameCapacity:frame_count];
  if (buffer == nil) {
    dispatch_semaphore_signal(ctx->queue_slots);
    return;
  }

  buffer.frameLength = frame_count;
  float* const* channel_data = buffer.floatChannelData;
  if (channel_data) {
    for (AVAudioChannelCount ch = 0; ch < ctx->format.channelCount; ch++) {
      const float* src = ctx->decode_buffer + ch;
      float* dst = channel_data[ch];
      for (AVAudioFrameCount i = 0; i < frame_count; i++) {
        dst[i] = src[i * ctx->channel_count];
      }
    }
  }

  dispatch_semaphore_t queue_slots = ctx->queue_slots;
  [ctx->player scheduleBuffer:buffer
       completionCallbackType:AVAudioPlayerNodeCompletionDataConsumed
            completionHandler:^(AVAudioPlayerNodeCompletionCallbackType callbackType) {
    (void)callbackType;
    dispatch_semaphore_signal(queue_slots);
  }];
}
