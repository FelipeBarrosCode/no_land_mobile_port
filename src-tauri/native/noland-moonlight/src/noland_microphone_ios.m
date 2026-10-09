// Native iOS capture/Opus/RTP path for No Land microphone forwarding.
#import <AVFoundation/AVFoundation.h>
#import <AudioToolbox/AudioToolbox.h>
#import <UIKit/UIKit.h>

#include "noland_moonlight.h"
#include <opus.h>

#include <netdb.h>
#include <netinet/in.h>
#include <errno.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>

#define NL_MIC_SAMPLE_RATE 48000
#define NL_MIC_RING_CAPACITY NL_MIC_SAMPLE_RATE
#define NL_MIC_RENDER_CAPACITY 8192
#define NL_MIC_PACKET_CAPACITY 1200

typedef struct {
  AudioComponentInstance audio_unit;
  OpusEncoder* encoder;
  int rtp_socket_fd;
  int rtcp_socket_fd;
  pthread_t worker;
  pthread_mutex_t mutex;
  pthread_cond_t condition;
  bool mutex_initialized;
  bool condition_initialized;
  float* ring;
  float* render_scratch;
  uint32_t ring_read;
  uint32_t ring_count;
  uint32_t frame_samples;
  uint16_t sequence;
  uint32_t timestamp;
  uint32_t ssrc;
  uint64_t payload_octets;
  char* host;
  uint16_t rtp_port;
  uint16_t rtcp_port;
  uint16_t local_rtcp_port;
  uint8_t reconnect_attempts;
  uint8_t supervision_reconnects;
  uint64_t last_supervised_rtcp_reports;
  __strong id interruption_observer;
  __strong id route_observer;
  __strong id background_observer;
  __strong id foreground_observer;
  atomic_bool running;
  atomic_bool app_active;
  atomic_bool interrupted;
  atomic_bool reconnect_requested;
  atomic_bool muted;
  atomic_ullong captured_samples;
  atomic_ullong encoded_packets;
  atomic_ullong sent_bytes;
  atomic_ullong dropped_samples;
  atomic_ullong rtcp_reports;
  atomic_ullong network_errors;
} nl_microphone_context_t;

static nl_microphone_context_t* g_microphone = NULL;
static pthread_mutex_t g_microphone_mutex = PTHREAD_MUTEX_INITIALIZER;
static int nl_microphone_connect_socket(const char* host, uint16_t port,
                                        uint16_t local_port);

static void nl_microphone_discard_queued_audio(nl_microphone_context_t* ctx) {
  pthread_mutex_lock(&ctx->mutex);
  ctx->ring_read = 0;
  ctx->ring_count = 0;
  pthread_mutex_unlock(&ctx->mutex);
}

static void nl_microphone_refresh_capture(nl_microphone_context_t* ctx) {
  if (ctx == NULL || ctx->audio_unit == NULL) return;
  bool should_capture = atomic_load(&ctx->running) &&
                        atomic_load(&ctx->app_active) &&
                        !atomic_load(&ctx->interrupted);
  if (should_capture) {
    AudioOutputUnitStart(ctx->audio_unit);
  } else {
    AudioOutputUnitStop(ctx->audio_unit);
    nl_microphone_discard_queued_audio(ctx);
  }
}

static bool nl_microphone_reconnect_sockets(nl_microphone_context_t* ctx) {
  if (ctx == NULL || ctx->host == NULL || ctx->reconnect_attempts >= 3) return false;
  ctx->reconnect_attempts++;
  if (ctx->rtp_socket_fd >= 0) close(ctx->rtp_socket_fd);
  if (ctx->rtcp_socket_fd >= 0) close(ctx->rtcp_socket_fd);
  ctx->rtp_socket_fd = -1;
  ctx->rtcp_socket_fd = -1;
  usleep((useconds_t)(250000U << (ctx->reconnect_attempts - 1)));
  ctx->rtp_socket_fd = nl_microphone_connect_socket(ctx->host, ctx->rtp_port, 0);
  ctx->rtcp_socket_fd = nl_microphone_connect_socket(
      ctx->host, ctx->rtcp_port, ctx->local_rtcp_port);
  if (ctx->rtp_socket_fd < 0 || ctx->rtcp_socket_fd < 0) {
    if (ctx->rtp_socket_fd >= 0) close(ctx->rtp_socket_fd);
    if (ctx->rtcp_socket_fd >= 0) close(ctx->rtcp_socket_fd);
    ctx->rtp_socket_fd = -1;
    ctx->rtcp_socket_fd = -1;
    return false;
  }
  ctx->reconnect_attempts = 0;
  return true;
}

static bool nl_microphone_has_permission(void) {
  AVAudioSession* session = AVAudioSession.sharedInstance;
  AVAudioSessionRecordPermission permission = session.recordPermission;
  if (permission == AVAudioSessionRecordPermissionGranted) return true;
  if (permission == AVAudioSessionRecordPermissionDenied) return false;

  __block atomic_bool finished = false;
  __block bool granted = false;
  [session requestRecordPermission:^(BOOL allowed) {
    granted = allowed;
    atomic_store(&finished, true);
  }];
  NSDate* deadline = [NSDate dateWithTimeIntervalSinceNow:30.0];
  while (!atomic_load(&finished) && deadline.timeIntervalSinceNow > 0) {
    if (NSThread.isMainThread) {
      [NSRunLoop.currentRunLoop runMode:NSDefaultRunLoopMode
                             beforeDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
    } else {
      usleep(50 * 1000);
    }
  }
  return atomic_load(&finished) && granted;
}

static bool nl_microphone_configure_audio_session(void) {
  AVAudioSession* session = AVAudioSession.sharedInstance;
  NSError* error = nil;
  AVAudioSessionCategoryOptions options =
      AVAudioSessionCategoryOptionDefaultToSpeaker |
      // AllowBluetoothHFP is the iOS 26 spelling of the long-standing HFP bit.
      // Use the compatible SDK spelling so CI and deployment SDKs both compile.
      (AVAudioSessionCategoryOptions)(1UL << 2);
  if (![session setCategory:AVAudioSessionCategoryPlayAndRecord
                         mode:AVAudioSessionModeVoiceChat
                      options:options
                        error:&error]) {
    return false;
  }
  [session setPreferredSampleRate:NL_MIC_SAMPLE_RATE error:nil];
  [session setPreferredIOBufferDuration:0.01 error:nil];
  return [session setActive:YES error:&error];
}

static void nl_microphone_restore_playback_session(void) {
  AVAudioSession* session = AVAudioSession.sharedInstance;
  [session setCategory:AVAudioSessionCategoryPlayback
           withOptions:AVAudioSessionCategoryOptionMixWithOthers
                 error:nil];
  [session setActive:YES error:nil];
}

static int nl_microphone_connect_socket(const char* host, uint16_t port,
                                        uint16_t local_port) {
  char service[8];
  snprintf(service, sizeof(service), "%u", port);
  struct addrinfo hints;
  memset(&hints, 0, sizeof(hints));
  hints.ai_family = AF_UNSPEC;
  hints.ai_socktype = SOCK_DGRAM;
  struct addrinfo* addresses = NULL;
  if (getaddrinfo(host, service, &hints, &addresses) != 0) return -1;

  int fd = -1;
  for (struct addrinfo* current = addresses; current != NULL; current = current->ai_next) {
    fd = socket(current->ai_family, current->ai_socktype, current->ai_protocol);
    if (fd < 0) continue;
    if (local_port != 0) {
      if (current->ai_family == AF_INET) {
        struct sockaddr_in local;
        memset(&local, 0, sizeof(local));
        local.sin_family = AF_INET;
        local.sin_port = htons(local_port);
        local.sin_addr.s_addr = htonl(INADDR_ANY);
        if (bind(fd, (struct sockaddr*)&local, sizeof(local)) != 0) {
          close(fd);
          fd = -1;
          continue;
        }
      } else if (current->ai_family == AF_INET6) {
        struct sockaddr_in6 local;
        memset(&local, 0, sizeof(local));
        local.sin6_family = AF_INET6;
        local.sin6_port = htons(local_port);
        local.sin6_addr = in6addr_any;
        if (bind(fd, (struct sockaddr*)&local, sizeof(local)) != 0) {
          close(fd);
          fd = -1;
          continue;
        }
      }
    }
    if (connect(fd, current->ai_addr, current->ai_addrlen) == 0) break;
    close(fd);
    fd = -1;
  }
  freeaddrinfo(addresses);
  return fd;
}

static void nl_write_u32(unsigned char* output, uint32_t value) {
  output[0] = (unsigned char)(value >> 24);
  output[1] = (unsigned char)(value >> 16);
  output[2] = (unsigned char)(value >> 8);
  output[3] = (unsigned char)value;
}

static void nl_microphone_send_rtcp_report(nl_microphone_context_t* ctx) {
  if (ctx == NULL || ctx->rtcp_socket_fd < 0) return;
  unsigned char report[28];
  memset(report, 0, sizeof(report));
  report[0] = 0x80;
  report[1] = 200;
  report[2] = 0;
  report[3] = 6;
  nl_write_u32(report + 4, ctx->ssrc);
  struct timeval now;
  gettimeofday(&now, NULL);
  uint32_t ntp_seconds = (uint32_t)now.tv_sec + 2208988800U;
  uint32_t ntp_fraction = (uint32_t)(((uint64_t)now.tv_usec << 32) / 1000000U);
  nl_write_u32(report + 8, ntp_seconds);
  nl_write_u32(report + 12, ntp_fraction);
  nl_write_u32(report + 16, ctx->timestamp);
  nl_write_u32(report + 20, (uint32_t)atomic_load(&ctx->encoded_packets));
  nl_write_u32(report + 24, (uint32_t)ctx->payload_octets);
  if (send(ctx->rtcp_socket_fd, report, sizeof(report), 0) < 0) {
    atomic_fetch_add(&ctx->network_errors, 1);
    atomic_store(&ctx->reconnect_requested, true);
  }

  unsigned char incoming[1200];
  ssize_t received;
  while ((received = recv(ctx->rtcp_socket_fd, incoming, sizeof(incoming), MSG_DONTWAIT)) > 0) {
    if (received >= 8 && incoming[0] >> 6 == 2 &&
        (incoming[1] == 200 || incoming[1] == 201)) {
      atomic_fetch_add(&ctx->rtcp_reports, 1);
    }
  }
}

static OSStatus nl_microphone_render_callback(
    void* ref_con,
    AudioUnitRenderActionFlags* flags,
    const AudioTimeStamp* timestamp,
    UInt32 bus_number,
    UInt32 frame_count,
    AudioBufferList* output) {
  (void)bus_number;
  (void)output;
  nl_microphone_context_t* ctx = (nl_microphone_context_t*)ref_con;
  if (ctx == NULL || !atomic_load(&ctx->running)) return noErr;
  if (frame_count > NL_MIC_RENDER_CAPACITY) {
    atomic_fetch_add(&ctx->dropped_samples, frame_count);
    return noErr;
  }

  AudioBufferList buffers;
  memset(&buffers, 0, sizeof(buffers));
  buffers.mNumberBuffers = 1;
  buffers.mBuffers[0].mNumberChannels = 1;
  buffers.mBuffers[0].mDataByteSize = frame_count * sizeof(float);
  buffers.mBuffers[0].mData = ctx->render_scratch;
  OSStatus result = AudioUnitRender(ctx->audio_unit, flags, timestamp, 1, frame_count, &buffers);
  if (result != noErr) return result;

  atomic_fetch_add(&ctx->captured_samples, frame_count);
  if (pthread_mutex_trylock(&ctx->mutex) != 0) {
    atomic_fetch_add(&ctx->dropped_samples, frame_count);
    return noErr;
  }
  for (UInt32 index = 0; index < frame_count; index++) {
    if (ctx->ring_count >= NL_MIC_RING_CAPACITY) {
      atomic_fetch_add(&ctx->dropped_samples, frame_count - index);
      break;
    }
    uint32_t write_index = (ctx->ring_read + ctx->ring_count) % NL_MIC_RING_CAPACITY;
    ctx->ring[write_index] = ctx->render_scratch[index];
    ctx->ring_count++;
  }
  pthread_cond_signal(&ctx->condition);
  pthread_mutex_unlock(&ctx->mutex);
  return noErr;
}

static void* nl_microphone_worker(void* opaque) {
  nl_microphone_context_t* ctx = (nl_microphone_context_t*)opaque;
  float* frame = calloc(ctx->frame_samples, sizeof(float));
  unsigned char packet[NL_MIC_PACKET_CAPACITY];
  if (frame == NULL) return NULL;

  while (atomic_load(&ctx->running)) {
    pthread_mutex_lock(&ctx->mutex);
    while (ctx->ring_count < ctx->frame_samples && atomic_load(&ctx->running)) {
      pthread_cond_wait(&ctx->condition, &ctx->mutex);
    }
    if (!atomic_load(&ctx->running)) {
      pthread_mutex_unlock(&ctx->mutex);
      break;
    }
    for (uint32_t index = 0; index < ctx->frame_samples; index++) {
      frame[index] = ctx->ring[ctx->ring_read];
      ctx->ring_read = (ctx->ring_read + 1) % NL_MIC_RING_CAPACITY;
    }
    ctx->ring_count -= ctx->frame_samples;
    pthread_mutex_unlock(&ctx->mutex);

    if (atomic_exchange(&ctx->reconnect_requested, false)) {
      ctx->reconnect_attempts = 0;
      nl_microphone_reconnect_sockets(ctx);
    }

    if (atomic_load(&ctx->muted)) memset(frame, 0, ctx->frame_samples * sizeof(float));
    pthread_mutex_lock(&ctx->mutex);
    int encoded = opus_encode_float(ctx->encoder, frame, (int)ctx->frame_samples,
                                    packet + 12, NL_MIC_PACKET_CAPACITY - 12);
    pthread_mutex_unlock(&ctx->mutex);
    if (encoded <= 0) continue;

    packet[0] = 0x80;
    packet[1] = 111;
    packet[2] = (unsigned char)(ctx->sequence >> 8);
    packet[3] = (unsigned char)ctx->sequence;
    packet[4] = (unsigned char)(ctx->timestamp >> 24);
    packet[5] = (unsigned char)(ctx->timestamp >> 16);
    packet[6] = (unsigned char)(ctx->timestamp >> 8);
    packet[7] = (unsigned char)ctx->timestamp;
    packet[8] = (unsigned char)(ctx->ssrc >> 24);
    packet[9] = (unsigned char)(ctx->ssrc >> 16);
    packet[10] = (unsigned char)(ctx->ssrc >> 8);
    packet[11] = (unsigned char)ctx->ssrc;
    ssize_t sent = send(ctx->rtp_socket_fd, packet, (size_t)encoded + 12, 0);
    if (sent > 0) {
      atomic_fetch_add(&ctx->encoded_packets, 1);
      atomic_fetch_add(&ctx->sent_bytes, (unsigned long long)sent);
      ctx->payload_octets += (uint64_t)encoded;
      if (atomic_load(&ctx->encoded_packets) % 100 == 0) {
        nl_microphone_send_rtcp_report(ctx);
      }
      if (atomic_load(&ctx->encoded_packets) % 1500 == 0) {
        uint64_t reports = atomic_load(&ctx->rtcp_reports);
        if (reports == ctx->last_supervised_rtcp_reports) {
          if (ctx->supervision_reconnects < 3) {
            ctx->supervision_reconnects++;
            atomic_store(&ctx->reconnect_requested, true);
          }
        } else {
          ctx->supervision_reconnects = 0;
          ctx->last_supervised_rtcp_reports = reports;
        }
      }
    } else {
      atomic_fetch_add(&ctx->network_errors, 1);
      nl_microphone_reconnect_sockets(ctx);
    }
    ctx->sequence++;
    ctx->timestamp += ctx->frame_samples;
  }
  free(frame);
  return NULL;
}

static void nl_microphone_release(nl_microphone_context_t* ctx) {
  if (ctx == NULL) return;
  if (ctx->interruption_observer != nil) {
    [NSNotificationCenter.defaultCenter removeObserver:ctx->interruption_observer];
    ctx->interruption_observer = nil;
  }
  if (ctx->route_observer != nil) {
    [NSNotificationCenter.defaultCenter removeObserver:ctx->route_observer];
    ctx->route_observer = nil;
  }
  if (ctx->background_observer != nil) {
    [NSNotificationCenter.defaultCenter removeObserver:ctx->background_observer];
    ctx->background_observer = nil;
  }
  if (ctx->foreground_observer != nil) {
    [NSNotificationCenter.defaultCenter removeObserver:ctx->foreground_observer];
    ctx->foreground_observer = nil;
  }
  if (ctx->audio_unit != NULL) {
    AudioUnitUninitialize(ctx->audio_unit);
    AudioComponentInstanceDispose(ctx->audio_unit);
  }
  if (ctx->encoder != NULL) opus_encoder_destroy(ctx->encoder);
  if (ctx->rtp_socket_fd >= 0) close(ctx->rtp_socket_fd);
  if (ctx->rtcp_socket_fd >= 0) close(ctx->rtcp_socket_fd);
  if (ctx->condition_initialized) pthread_cond_destroy(&ctx->condition);
  if (ctx->mutex_initialized) pthread_mutex_destroy(&ctx->mutex);
  free(ctx->ring);
  free(ctx->render_scratch);
  free(ctx->host);
  free(ctx);
  nl_microphone_restore_playback_session();
}

void nl_microphone_stop(void) {
  pthread_mutex_lock(&g_microphone_mutex);
  nl_microphone_context_t* ctx = g_microphone;
  g_microphone = NULL;
  pthread_mutex_unlock(&g_microphone_mutex);
  if (ctx == NULL) return;

  atomic_store(&ctx->running, false);
  if (ctx->audio_unit != NULL) AudioOutputUnitStop(ctx->audio_unit);
  pthread_mutex_lock(&ctx->mutex);
  pthread_cond_broadcast(&ctx->condition);
  pthread_mutex_unlock(&ctx->mutex);
  pthread_join(ctx->worker, NULL);
  nl_microphone_release(ctx);
}

int nl_microphone_start(const char* host, uint16_t rtp_port,
                         uint16_t rtcp_port, uint16_t local_rtcp_port,
                         uint32_t ssrc,
                         uint16_t sequence_offset, uint32_t timestamp_offset,
                         uint32_t bitrate_bps, uint32_t frame_ms) {
  if (host == NULL || host[0] == '\0' || rtp_port == 0 || rtcp_port == 0 ||
      local_rtcp_port == 0 || frame_ms != 10) return -1;
  if (!nl_microphone_has_permission()) return -2;
  nl_microphone_stop();
  if (!nl_microphone_configure_audio_session()) return -6;

  nl_microphone_context_t* ctx = calloc(1, sizeof(*ctx));
  if (ctx == NULL) return -3;
  ctx->rtp_socket_fd = -1;
  ctx->rtcp_socket_fd = -1;
  ctx->frame_samples = (NL_MIC_SAMPLE_RATE * frame_ms) / 1000;
  if (ctx->frame_samples == 0 || ctx->frame_samples > 2880) { free(ctx); return -4; }
  ctx->sequence = sequence_offset;
  ctx->timestamp = timestamp_offset;
  ctx->ssrc = ssrc;
  ctx->host = strdup(host);
  ctx->rtp_port = rtp_port;
  ctx->rtcp_port = rtcp_port;
  ctx->local_rtcp_port = local_rtcp_port;
  if (ctx->host == NULL) goto fail;
  atomic_store(&ctx->app_active,
               UIApplication.sharedApplication.applicationState == UIApplicationStateActive);
  atomic_store(&ctx->interrupted, false);
  ctx->ring = calloc(NL_MIC_RING_CAPACITY, sizeof(float));
  ctx->render_scratch = calloc(NL_MIC_RENDER_CAPACITY, sizeof(float));
  if (ctx->ring == NULL || ctx->render_scratch == NULL) goto fail;
  if (pthread_mutex_init(&ctx->mutex, NULL) != 0) goto fail;
  ctx->mutex_initialized = true;
  if (pthread_cond_init(&ctx->condition, NULL) != 0) goto fail;
  ctx->condition_initialized = true;

  int opus_error = OPUS_OK;
  ctx->encoder = opus_encoder_create(NL_MIC_SAMPLE_RATE, 1, OPUS_APPLICATION_VOIP, &opus_error);
  if (ctx->encoder == NULL || opus_error != OPUS_OK) goto fail;
  opus_encoder_ctl(ctx->encoder, OPUS_SET_BITRATE((int)bitrate_bps));
  opus_encoder_ctl(ctx->encoder, OPUS_SET_INBAND_FEC(1));
  opus_encoder_ctl(ctx->encoder, OPUS_SET_PACKET_LOSS_PERC(5));
  opus_encoder_ctl(ctx->encoder, OPUS_SET_DTX(0));
  ctx->rtp_socket_fd = nl_microphone_connect_socket(host, rtp_port, 0);
  if (ctx->rtp_socket_fd < 0) goto fail;
  ctx->rtcp_socket_fd = nl_microphone_connect_socket(host, rtcp_port, local_rtcp_port);
  if (ctx->rtcp_socket_fd < 0) goto fail;

  AudioComponentDescription description = {
    .componentType = kAudioUnitType_Output,
    .componentSubType = kAudioUnitSubType_RemoteIO,
    .componentManufacturer = kAudioUnitManufacturer_Apple,
  };
  AudioComponent component = AudioComponentFindNext(NULL, &description);
  if (component == NULL || AudioComponentInstanceNew(component, &ctx->audio_unit) != noErr) goto fail;
  UInt32 enabled = 1;
  if (AudioUnitSetProperty(ctx->audio_unit, kAudioOutputUnitProperty_EnableIO,
                           kAudioUnitScope_Input, 1, &enabled, sizeof(enabled)) != noErr) goto fail;
  AudioStreamBasicDescription format;
  memset(&format, 0, sizeof(format));
  format.mSampleRate = NL_MIC_SAMPLE_RATE;
  format.mFormatID = kAudioFormatLinearPCM;
  format.mFormatFlags = kAudioFormatFlagsNativeFloatPacked;
  format.mFramesPerPacket = 1;
  format.mChannelsPerFrame = 1;
  format.mBytesPerFrame = sizeof(float);
  format.mBytesPerPacket = sizeof(float);
  format.mBitsPerChannel = 32;
  if (AudioUnitSetProperty(ctx->audio_unit, kAudioUnitProperty_StreamFormat,
                           kAudioUnitScope_Output, 1, &format, sizeof(format)) != noErr) goto fail;
  AURenderCallbackStruct callback = {
    .inputProc = nl_microphone_render_callback,
    .inputProcRefCon = ctx,
  };
  if (AudioUnitSetProperty(ctx->audio_unit, kAudioOutputUnitProperty_SetInputCallback,
                           kAudioUnitScope_Global, 1, &callback, sizeof(callback)) != noErr) goto fail;
  if (AudioUnitInitialize(ctx->audio_unit) != noErr) goto fail;

  atomic_store(&ctx->running, true);
  if (pthread_create(&ctx->worker, NULL, nl_microphone_worker, ctx) != 0) {
    atomic_store(&ctx->running, false);
    goto fail;
  }
  if (atomic_load(&ctx->app_active) && AudioOutputUnitStart(ctx->audio_unit) != noErr) {
    atomic_store(&ctx->running, false);
    pthread_mutex_lock(&ctx->mutex);
    pthread_cond_broadcast(&ctx->condition);
    pthread_mutex_unlock(&ctx->mutex);
    pthread_join(ctx->worker, NULL);
    goto fail;
  }

  pthread_mutex_lock(&g_microphone_mutex);
  g_microphone = ctx;
  pthread_mutex_unlock(&g_microphone_mutex);
  ctx->interruption_observer = [NSNotificationCenter.defaultCenter
      addObserverForName:AVAudioSessionInterruptionNotification
                  object:nil
                   queue:NSOperationQueue.mainQueue
              usingBlock:^(NSNotification* note) {
    NSNumber* raw = note.userInfo[AVAudioSessionInterruptionTypeKey];
    pthread_mutex_lock(&g_microphone_mutex);
    if (g_microphone == ctx && atomic_load(&ctx->running)) {
      if (raw.unsignedIntegerValue == AVAudioSessionInterruptionTypeBegan) {
        atomic_store(&ctx->interrupted, true);
      } else {
        if (nl_microphone_configure_audio_session()) {
          atomic_store(&ctx->interrupted, false);
          ctx->supervision_reconnects = 0;
          atomic_store(&ctx->reconnect_requested, true);
        }
      }
      nl_microphone_refresh_capture(ctx);
    }
    pthread_mutex_unlock(&g_microphone_mutex);
  }];
  ctx->route_observer = [NSNotificationCenter.defaultCenter
      addObserverForName:AVAudioSessionRouteChangeNotification
                  object:nil
                   queue:NSOperationQueue.mainQueue
              usingBlock:^(NSNotification* note) {
    (void)note;
    pthread_mutex_lock(&g_microphone_mutex);
    if (g_microphone == ctx && atomic_load(&ctx->running)) {
      nl_microphone_configure_audio_session();
      ctx->supervision_reconnects = 0;
      atomic_store(&ctx->reconnect_requested, true);
      nl_microphone_refresh_capture(ctx);
    }
    pthread_mutex_unlock(&g_microphone_mutex);
  }];
  ctx->background_observer = [NSNotificationCenter.defaultCenter
      addObserverForName:UIApplicationWillResignActiveNotification
                  object:nil
                   queue:NSOperationQueue.mainQueue
              usingBlock:^(NSNotification* note) {
    (void)note;
    pthread_mutex_lock(&g_microphone_mutex);
    if (g_microphone == ctx && atomic_load(&ctx->running)) {
      atomic_store(&ctx->app_active, false);
      nl_microphone_refresh_capture(ctx);
    }
    pthread_mutex_unlock(&g_microphone_mutex);
  }];
  ctx->foreground_observer = [NSNotificationCenter.defaultCenter
      addObserverForName:UIApplicationDidBecomeActiveNotification
                  object:nil
                   queue:NSOperationQueue.mainQueue
              usingBlock:^(NSNotification* note) {
    (void)note;
    pthread_mutex_lock(&g_microphone_mutex);
    if (g_microphone == ctx && atomic_load(&ctx->running)) {
      atomic_store(&ctx->app_active, true);
      nl_microphone_configure_audio_session();
      ctx->supervision_reconnects = 0;
      atomic_store(&ctx->reconnect_requested, true);
      nl_microphone_refresh_capture(ctx);
    }
    pthread_mutex_unlock(&g_microphone_mutex);
  }];
  return 0;

fail:
  nl_microphone_release(ctx);
  return -5;
}

void nl_microphone_set_muted(bool muted) {
  pthread_mutex_lock(&g_microphone_mutex);
  if (g_microphone != NULL) atomic_store(&g_microphone->muted, muted);
  pthread_mutex_unlock(&g_microphone_mutex);
}

int nl_microphone_set_bitrate(uint32_t bitrate_bps) {
  int result = -1;
  pthread_mutex_lock(&g_microphone_mutex);
  if (g_microphone != NULL && g_microphone->encoder != NULL && bitrate_bps > 0) {
    pthread_mutex_lock(&g_microphone->mutex);
    result = opus_encoder_ctl(g_microphone->encoder, OPUS_SET_BITRATE((int)bitrate_bps));
    pthread_mutex_unlock(&g_microphone->mutex);
  }
  pthread_mutex_unlock(&g_microphone_mutex);
  return result;
}

void nl_microphone_get_statistics(nl_microphone_statistics_t* statistics) {
  if (statistics == NULL) return;
  memset(statistics, 0, sizeof(*statistics));
  pthread_mutex_lock(&g_microphone_mutex);
  nl_microphone_context_t* ctx = g_microphone;
  if (ctx != NULL) {
    statistics->captured_samples = atomic_load(&ctx->captured_samples);
    statistics->encoded_packets = atomic_load(&ctx->encoded_packets);
    statistics->sent_bytes = atomic_load(&ctx->sent_bytes);
    statistics->dropped_samples = atomic_load(&ctx->dropped_samples);
    statistics->rtcp_reports = atomic_load(&ctx->rtcp_reports);
    statistics->network_errors = atomic_load(&ctx->network_errors);
    pthread_mutex_lock(&ctx->mutex);
    statistics->queue_depth_samples = ctx->ring_count;
    pthread_mutex_unlock(&ctx->mutex);
    statistics->running = atomic_load(&ctx->running) ? 1 : 0;
    statistics->suspended = (!atomic_load(&ctx->app_active) ||
                             atomic_load(&ctx->interrupted)) ? 1 : 0;
    statistics->muted = atomic_load(&ctx->muted) ? 1 : 0;
  }
  pthread_mutex_unlock(&g_microphone_mutex);
}
