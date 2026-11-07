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

## Discrete-FFT element

An `FFT` element with `fftStyle=discrete`: a 2^9 = 512-point transform with a Bartlett window
and scaling 1.0, one spectrum per 512-sample block (1.024 s at 500 Hz), followed by a
`VectorAggregator` that averages 3 absolute spectra.

## Not yet recovered

The runtime algorithms (IIR band-pass amplitude, discrete aggregation, discrete FFT and
`VectorAggregator` absolute) are unverified here, and no replay capture of the original running
this design exists: the RTTI scan found no class names for these elements, so their vtables
were not located, and the guest could not be driven to select the design.
