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
- `DSPFFT::process` `0x637940`: `float32` Bartlett window (`sensor-status.md`, "Window": four SSE
  lanes, mean 0.499022156 as the sequential `float32` sum / 512), the IPP order-9 real FFT
  (`ipp_fft.rs`), the IPP magnitude (`rsqrtps` plus one Newton step, `ipp_magnitude.rs`) of bins
  0..255, then x87 `f32(mag * (1/128) / mean)`. `feedback::DspFft` is this chain, bit exact per
  block against the sensors-off capture (`sensor-status.md`, "Level bit exactness").
- **Trigger count 500 (observed, #59 tag 30).** The element gathers 500 samples (`+0x73`), not
  512: every logged call has input samples 0..499 and zeros from 500 on. Exercise 147's 120020
  samples are 240 whole blocks plus a 20-sample partial, 241 calls per channel, the last one at
  the stop.
- Stored output: one 256-value vector per channel at x = the stop (240040 ms for exercise 147),
  the absolute `VectorAggregator` emitting once, at the stop. It is the mean of all 241 logged
  outputs (relative error about 2e-8, the outputs being `float32`), partial block included. The
  input is the Source output as stored (`f32(trunc(x*d)/d)`).

## Clone and parity

`feedback::FftSpectrum` implements the above over the stored Source output, through `DspFft`, and
`StoredSeries` (`assess.rs`) stores its mean as each channel's `FFT-Normal-Order` series (`x` the
bin index, `y` microvolts) next to the Band 10 and Band 80 series: the clone's BAGSD report.

`tests/replay_s59.rs` (ignored; `S59_REPLAY_DIR`, default `tmp/s59/replay`) runs the #59 capture
(exercise 147, `replay-rig.md`) through the Source chain and `StoredSeries` and holds every
channel's Band 10, Band 80 and `FFT-Normal-Order` to the stored record with
`assertions::stored_series`. Observed: green, largest |dy| 3.8e-7 uV against `RECORD_EPSILON`
1e-6. The graph start (consumed sample 107196) and the 45 Hz low-pass switch (the same sample, on
every channel) are the logged inputs the clone does not derive; both were located by matching
the first logged `DSPFFT` input block against the clone's Source output, which it equals for all
500 samples on every channel.

What closed the earlier 2 to 3 percent gap (#59's first pass on exercise 146): the 500-sample
trigger (the old 512 block put the zero padding in the wrong place), the original's window and
mean (the f64 ramp is 1 to 3 ulp off), and the x87 scale. The last 1 to 3 `float32` ulp per
block was the magnitude: with #57's IPP `rsqrtps` magnitude, `DspFft` equals all 964 logged
`DSPFFT` outputs of exercise 147 (241 calls on each of 4 channels) bit for bit, asserted by
`tests/replay_s59.rs` (`fft_in_<c>.txt` / `fft_out_<c>.txt` from `scripts/fft-extract.py`).
