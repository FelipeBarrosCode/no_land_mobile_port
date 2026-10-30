// Source-only provider/native fixtures: intentionally distinct offer, host and machine IDs.
export const nativeOffer = (machineId = "9007199254740993") => ({
  id: 345,
  machineId,
  hostId: 678,
  hostLabel: "Fixture host",
  gpuName: "Fixture GPU",
  reliability: 0.98,
  score: 42,
  hourlyPrice: 0.25,
});

export const unknownMachineOffer = {
  id: 123,
  hostId: 456,
  hostLabel: "Host-789",
  reliability: 0.99,
  score: 60,
  hourlyPrice: 0.3,
};
