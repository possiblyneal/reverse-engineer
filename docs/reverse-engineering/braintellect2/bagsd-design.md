# BAGSD-4-chnl-wDiscreteFFT-v5.1.0 design graph

Ticket #59. Decoded statically from the session database (`flowdesign.design`, design id 172,
350426 bytes of flow-design XML after AES-128-CFB decryption; see `session-storage.md`).
`configuration.designRole` is `BAGSD.generate`; the exercise is stored as `exerciseBAGSData`.

## Element graph (static, observed)

204 connections, all from `Source.Out1` through one filter into one collector pipe per series.

| Element class | Count | Properties |
|---|---|---|
| `LowPassFilter` | 4 | Butterworth, `filterResponse=infinite`, order 1, `highFrequency` 1 |
| `BandPassFilter` | 56 | same, bands (Hz): 1-3, 3-5.5, 5.5-7.5, 7.5-10, 10-12, 12-15, 15-23, 23-36, 36-48 per channel (10 band slots per channel counting the low-pass) |
| `HighPassFilter` | 4 | same, `lowFrequency` 62 |
| `AggregatorFilter` | 64 | `flowStyle=discrete`, `averageEpoch=5`, one after every filter |
| `FFT` | 4 | `fftStyle=discrete`, `fftOrder=9`, `windowType=Bartlett`, `fftScalingFactor=1.0` |
| `VectorAggregator` | 4 | `flowStyle=absolute`, `averageEpoch=3`, one after every FFT |

Channels 1-2 use the `NN chN` names, channels 3-4 the `NN ch N` / `VHF` names. Each channel
chain is `Source -> filter -> AggregatorFilter -> GeneralDataCollector`; the FFT chain is
`Source -> FFT -> VectorAggregator -> collector`. Raw property dump: the decoded XML is
sensitive-free but large, so it is not committed.

## Run by the original after the exercise

`designRole` `BAGSD.generate` marks a post-session generator: the original runs it over the
stored Source data after every SMART-2 exercise (#56 captured that run), so the session window
cannot select it and a live session cannot run it. Same finding as Circle-Balance (#62); the
clone has no selector for it and none is invented. Its outputs are the stored series of
`session-storage.md`: the aggregator bands (#56) and one `FFT-Normal-Order` vector per channel.

## Discrete-FFT element (observed, static)

- Receive `0x6382d0`: a 512-float buffer (`+0x72` size, `+0x73` trigger count). A type-7 block
  appends, or slides the buffer if it would overflow. Discrete (`+0x74` nonzero) transforms
  only once the count reaches `+0x73`, then zeroes the buffer. A start or stop message (type 2
  or 0xc) transforms a partial buffer, zero padded, when its count is neither 0 nor `+0x73`.
- `DSPFFT::process` `0x637940`: `float32` Bartlett window, the IPP order-9 real FFT
  (`ipp_fft.rs`), the IPP magnitude (`rsqrtps` plus one Newton step, `ipp_magnitude.rs`) of
  bins 0..255, scaled by `1/128` over the window mean (0.499022156). `feedback::DspFft` is
  this chain, bit exact per block against the sensors-off capture (`sensor-status.md`, "Level
  bit exactness").
- Stored output: one 256-value vector per channel at x = 240016 ms (the stop), consistent with
  the absolute `VectorAggregator` emitting the mean of the block spectra once, at the stop. The
  input is the Source output as stored (`f32(trunc(x*d)/d)`): 120008 samples from the graph
  start, 234 whole blocks plus a 200-sample partial.

## Clone and parity

`feedback::FftSpectrum` implements the above over the stored Source output, through `DspFft`.
`tests/replay_s59.rs` (ignored; `S59_REPLAY_DIR`, default `tmp/s56`) runs exercise 146's
consumed volts through it and reports the deviation from the stored vectors. The capture is
#56's stored type-9 record; no run of this ticket logged `DSPFFT` inputs and outputs.

Reported figure, not parity: mean relative deviation over bins 0..48 is 2 to 3 percent, and the
high-bin floor is a third of the original's (bin 200: 0.00016 against 0.0005 uV on channel 1).
Ruled out offline: window shape, with and without the partial block, contiguous block subsets
or the last 1 and 3 blocks, input quantization, and the 45 Hz low-pass switch (without it the
spectrum is far off). `assertions::fft_spectra` holds the stage to `RECORD_EPSILON` once an
input/output log of `DSPFFT::process` (rig hook tag 30 of `s57/hook13.js`) explains the floor.
