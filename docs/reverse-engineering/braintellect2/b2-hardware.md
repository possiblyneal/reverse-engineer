# B-2 hardware: board parts, battery, access points

What the bare B-2 board (silkscreen `BRAINTELLECT B2v2`, assembly `090-9018-F`, bare board
`091-9018-F`) is built from, tied to the behaviour in `b2-protocol.md`. Sources are the Operator's
photos of one board (kept outside Git, 15 files) and vendor datasheets.

Tags: **observed** = read from a photo (named in brackets) or a datasheet (section named);
**inferred** = a reading the source does not state. Photo names: `top-full`, `top-full-rotated`,
`top-closeup-right`, `bottom`, `battery-label`, `u60-q50-j56`, `q50-u61-j57`, `u52-vbata-u60`,
`opamps-j50`, `opamps-j50-left`, `wt12-j61`, `stm32-y51-j52`, `stm32-swd-buttons`,
`ads1294-z51-u53`, `j59-batt-j58`, plus `scan-bottom`, a 600 dpi flatbed scan of the bottom side
(236.22 px/cm, so distances on it are measurable). Chip lot or date codes, the WT12 label's lot line and
certification text, the battery date code and the white sticker areas are left out on purpose.

## Parts

| Ref | Marking | Role | Tag and source |
|---|---|---|---|
| U54 | TI `ADS1294` (64-pin TQFP, no `R` suffix) | 4-channel 24-bit delta-sigma ADC with PGA, internal reference, SPI | marking observed [`ads1294-z51-u53`, `top-full`]; role observed (ADS129x datasheet SBAS459K, features: 4 channels, 24 bit, PGA gain 1, 2, 3, 4, 6, 8, 12) |
| Z51 | `2.048M` oscillator, 4-pad can | ADS1294 master clock; `CLK` test point beside it | marking observed [`ads1294-z51-u53`]; role inferred (2.048 MHz is the datasheet's external `fCLK`) |
| U57 | ST `STM32 F071CBT6` (LQFP48) | host MCU: SPI master to U54, UART to Z50, SWD on J54 | marking observed [`stm32-y51-j52`]; no USB peripheral observed (STM32F071x8/xB datasheet feature list: I2C, USART, SPI, CEC, SWD, ADC; USB appears only on the F072 family); roles inferred |
| Y51 | `ATS08ASM-E` crystal, HC-49 SMD | MCU clock, 8 MHz | marking observed [`stm32-y51-j52`]; 8 MHz inferred from the CTS ATS-SM naming (`ATS08ASM-1E` is listed as 8 MHz) |
| Z50 | Bluegiga/Silicon Labs `WT12-A` | Bluetooth 2.1+EDR module (BlueCore04), carries the RFCOMM link | marking observed [`wt12-j61`]; role per WT12 datasheet (UART, SPI, PCM, USB, 6 PIO) |
| U50, U51, U52, U55, U59 | TI `OPA 2376` (SOIC-8) | dual low-noise rail-to-rail op-amps, 2.2-5.5 V, 7.5 nV/rtHz | marking observed [`opamps-j50`, `opamps-j50-left`, `u52-vbata-u60`, `top-full`]; U50-U52 sit by J50 with `BIAS`, `REF`, `FP1`, `FP2`, `T3`, `T4` test points; use as input buffers and bias or reference drive inferred (OPA2376 datasheet, applications: ADC buffer, medical instrumentation) |
| J50 | full-size HDMI Type A receptacle (19 SMT tails in one row), board edge | electrode or sensor cable | 19 tails observed [`opamps-j50-left`]; Type A from the shell tabs, 15.7 mm apart on `scan-bottom` (Mini-HDMI Type C is about 10.5 mm wide); role inferred (the input network feeds U50-U52); pin map not traced, see "Tracing" |
| J57 | micro-USB receptacle | charging | observed; "charging only" inferred: U57 has no USB peripheral and the receptacle sits next to U61 and the battery path [`q50-u61-j57`] |
| J59 | white 2-pin plug housing, silkscreen `BATT` | battery connector | observed [`j59-batt-j58`] |
| U60 | `12F1840` (Microchip PIC12F1840, SOIC-8) | 8-bit MCU on the power path | marking observed [`u60-q50-j56`, `u52-vbata-u60`]; role unresolved, see "U60" |
| Q50 | Diodes `P2038US` (SOIC-8) = DMP2038USS | -20 V P-channel MOSFET, 38 mOhm at VGS -4.5 V | marking observed [`u60-q50-j56`, `q50-u61-j57`]; part per the Diodes DMP2038USS datasheet (marking table: `P2038US` = product type code); load switch on VBAT inferred from position and the `VBAT`/`VBATA` split |
| Q51 | `N20` (SOT-23), beside the push button and R584 | small N-channel switch or digital transistor | unresolved: the code `N20` is reused by unrelated parts (candidates from a marking-code database: pre-biased NPN transistor, 20 V N-channel MOSFET, 2.0 V voltage supervisor) |
| U53 | `B55Q` (SOT-23-5), by the `AVDD` and `VBATA` test points | probably the analog supply LDO | marking observed [`ads1294-z51-u53`]; unresolved: no exact marking-code match (nearest codes `8550` LDO/PNP, `B550` voltage supervisor); LDO role inferred from position and the L50/C53x filter |
| U56 | `B55Q` (SOT-23-5), by the `3.3V` test point | probably the 3.3 V LDO | as U53 [`wt12-j61`] |
| U58 | `PKKP` (SOT-23-5), by SWD and `RESET` | unidentified | observed [`stm32-swd-buttons`]; unresolved, no marking-code match; candidates: reset supervisor, ESD or load switch |
| U61 | `KE87` (SOT-23-5), by J57 | probably the Li-ion charger | observed [`q50-u61-j57`]; unresolved, no marking-code match; charger role inferred from J57 plus the battery path |
| S51, S52, S53 | `PB-1`, `PB-2`, `RESET` tactile-switch footprints, switches not seen fitted | user buttons and MCU reset | silkscreen observed [`top-closeup-right`, `stm32-swd-buttons`] |
| DS51, DS52 and a side light pipe | `LED-1`, `LED-2` | status LEDs | observed [`stm32-y51-j52`, `q50-u61-j57`] |
| (side) | black push button by `GND`, solder tabs | power or user button | observed [`u60-q50-j56`]; function inferred |

The ADS1294 is a 4-channel part, while the app reports 8 channel slots as the "hardware maximum"
and the 131-138 replies cover 8 slots with 135-138 inactive (`b2-protocol.md`). The 8-slot
software model is therefore wider than this board (inferred: the same family code accepts the
8-channel ADS1298).

## Headers, test points and jumpers

All observed from the photos named; function of each is inferred unless stated.

| Ref | What | Source |
|---|---|---|
| J54 `SWD` | 2x3 pad footprint with 3 alignment holes (Tag-Connect style), unpopulated | `stm32-swd-buttons` |
| J56 | 5-pin through-hole row beside U60, square pad at the right end; ICSP header for U60 inferred (PIC12F1840 ICSP needs MCLR, VDD, VSS, ICSPDAT on pin 7, ICSPCLK on pin 6) | `u60-q50-j56` |
| J61 | 2x3 through-hole header beside Z50, pin-1 square pad bottom left; BlueCore SPI (NCSB 21, SCLK 22, MISO 23, MOSI 24 on the WT12) inferred, trace to the module pins not followed | `wt12-j61` |
| J52 | 4 through-holes above Z50, pin 1 the square pad at the right end (board text upright): pin 1 signal, pin 2 ground (`DGND`), pin 3 `3.3V`, pin 4 signal; neither signal reaches `SDA2` or `SCL2`, and each reads about 10 MOhm to pin 3 unpowered (no pull-up resistor, so not I2C); powered and idle, pins 1 and 4 both read 3.3 V to pin 2, as idle UART lines do (a TX line drives high, an RX line with the MCU's internal pull-up floats high), so which is TX is not settled; a UART debug or programming port (TX, RX, 3.3 V, GND) inferred | continuity, Operator meter; square pad from `top-full` |
| J53 `AUX` | 2x4 pad block plus 14-pin FPC footprint, expansion | `stm32-y51-j52` |
| J55 | 2-pin `-`/`+` footprint, mid-board, beside U55; unpowered, neither pin beeps to `DGND` and `-` does not beep to `REF`, 13.7 MOhm between the pins. Powered, `-` reads 2.025 V (the `REF` level) and `+` 1.93 V (the `TESTSIG` level), and unpowered `+` beeps to `TESTSIG` and to both `J62` holes (all four points one net, about 1.93 V powered), so a test-signal output referenced to `REF` is inferred, possibly the signal behind the protocol's injection command (`b2-protocol.md`) | `top-full`; Operator meter | `top-full`; continuity, Operator meter |
| J62 | 2 holes, both on the `TESTSIG` net (see J55) | `top-full`; Operator meter |
| J58 `VBAT`, J60 `DGND`, J51 `AGND` | 2-pin power or ground taps; `AGND` and `DGND` beep together unpowered (joined on the board, directly or through a low-value part) | `j59-batt-j58`, `wt12-j61`, `opamps-j50-left`, `stm32-y51-j52` |
| Test pads, ADS SPI | `CS/`, `MISO`, `MOSI`, `SCK`, `RDY/` beside Z51 | `ads1294-z51-u53` |
| Test pads, other | `CLK`, `MCO`, `SDA2`, `SCL2`, `BIAS`, `REF`, `FP1`, `FP2`, `T1`-`T4`, `SO`, `SI`, `TESTSIG`; WT12 `RTS` and `CTS` | `top-full`, `stm32-y51-j52`, `wt12-j61` |
| Rails | `VBAT`, `VBATA`, `VBATA/2`, `AVDD`, `3.3V` (two), `VDDA` | `top-full`, `j59-batt-j58` |
| Jumpers | `JMP50` (3 pads left of `AVDD`): open, nothing fitted (observed, Operator confirmed). `JMP52`, `JMP53` (2 pads each, between Z51 and U57): closed by a 0 Ω part (black, printed `0`; Operator confirmed JMP52). `RJMP50` (by J50), `RJMP54` (by `CLK`), `RJMP53` (by U57): 3-pad `A`/`B` selectors, each fitted on the `A` side with the `B` pad bare; RJMP50 and RJMP54 carry a 0 Ω part, RJMP53 a resistor marked `01C` (10 kΩ in the EIA-96 code). RJMP53 sits by a `B0` label, so a BOOT0 pull resistor is inferred; which rail `A` ties to is not traced | `top-full`, `opamps-j50-left` |
| Buttons | `PB-1`, `PB-2`, `RESET` footprints unpopulated | `stm32-swd-buttons` |

`VBATA/2` is labelled as a half-voltage test point, which suggests a divider for battery sensing
(inferred; the ADC input it feeds is not traced).

## Tracing

What the photos and the bottom scan can and cannot show.

- **At least four layers** (observed, `scan-bottom`): the bottom side is almost all a hatched copper
  pour with a few short traces, and most vias that carry a top trace have no bottom trace. Every via
  the Operator checked goes through the board, so the routing between vias is on inner layers that
  no photo shows.
- **J50** (observed, `opamps-j50-left`): top traces leave roughly 7-8 of the 19 tails and each ends
  at a via within a few millimetres, toward the RJMP50 `A` pad and the C50x/C52x input capacitors.
  Neither the tail count per trace nor the pin numbering direction can be read reliably, so the
  pin-to-net map (`FP1`, `FP2`, `T3`, `T4`, `REF`, `BIAS`, `AGND`, unused) needs a continuity meter
  on the unpowered board: one probe on each J50 tail (or the matching contact inside the receptacle),
  the other on each test point and on `AGND` (J51).
- The same limit applies to J52, J56 and J61 and to the STM32-WT12 UART: a net that dives through a
  via is a meter job, not a photo job.

## Powered readings

Operator meter, DC volts to J52 pin 2 (ground), battery on `BATT`, board on but not streaming
(observed; one board, one battery charge). With no host connected the board switched itself off
during the readings, so a zero is only trusted where it was rechecked with the board on.

| Point | Volts | Reading |
|---|---|---|
| `VBAT` | 4.06 | battery, near full |
| `VBATA` | 4.05 | VBAT through Q50 (switched on, about 10 mV drop) |
| `VBATA/2` | 2.03 | half of VBATA: a battery-sense divider |
| `REF` | 2.03 | equals VBATA/2, so the electrode reference is inferred to be the buffered mid-battery voltage |
| `FP1`, `FP2`, `T3`, `T4` | 2.025 | at `REF`: these four inputs (by U50-U52) sit at the mid-battery reference |
| `T1`, `T2` | 1.012 | half of `REF` (VBATA/4): a different bias, on the U59 side of the board |
| `SI`, `SO` | 0 | idle (rechecked with the board confirmed on) |
| `TESTSIG`, J55 `+`, both `J62` holes | 1.93 | one net (continuity); DC level only, a test signal on it would need a scope to see |
| J55 `-` | 2.025 | the `REF` level |
| `AVDD` | 3.3 | ADS1294 analog supply |
| `BIAS` | 0 | bias drive off while idle (inferred: driven only during a session) |
| J52 pins 1, 4 | 3.3 | both serial lines idle high |
| J61 | 3.3 on the top-right and bottom-left holes, 0 on the other four; unpowered, top-left beeps to ground and bottom-left to `3.3V` | top-left ground, bottom-left 3.3 V supply, top-right an idle-high signal (BlueCore SPI `CSB` inferred), the other three signals (`MOSI`, `MISO`, `CLK` inferred) |
| J56, pin 1 (square pad, bottom) to pin 5 | 4.04, 4.04, 0, 0, 4.0 | matches the Microchip ICSP order (1 `MCLR` high, 2 `VDD`, 3 `VSS`, 4 `ICSPDAT`, 5 `ICSPCLK`) on the battery rail, not 3.3 V; pin 3 beeps to `DGND` unpowered; U60 inferred to be the always-on power controller |

## ADS1294 against the protocol constants

Datasheet facts (ADS129x SBAS459K): full-scale differential input is plus or minus VREF/gain;
internal VREF is 2.4 V (`CONFIG3.VREF_4V` = 0, AVDD >= 2.7 V) or 4 V (`VREF_4V` = 1, AVDD >= 4.4 V);
PGA gains 1, 2, 3, 4, 6, 8, 12; codes are 24-bit two's complement, so one LSB is
VREF/(gain x 2^23) (the Linux `ti-ads1298` driver computes its scale as VREF/gain over 2^23).

`b2-protocol.md` volts = `rawADC x D1 / (channelGain x D2 x sensorGain)` with, for family 12,
D1 = 2.4 V and D2 = 2^23 - 1.

- **Agreement (arithmetic).** D1 = 2.4 V is the internal reference; D2 = 8388607 is the 24-bit
  positive full-scale count (2^23 - 1 versus the datasheet's 2^23, a relative difference of
  1.2e-7, far below the 0.2 % reference accuracy). So D1/D2 = 2.861e-7 V per count at gain 1 is
  the VREF/(2^23 - 1) form. Family 99 (D1 = 4.0 V) matches the 4 V reference option; family 11
  (2^15 - 1) matches a 16-bit part such as the ADS1194 family, not this board.
- **channelGain is the PGA gain.** 12 is a valid ADS1294 setting (`CHnSET` GAIN bits 110 in the
  `ti-ads1298` driver's table `{6, 1, 2, 3, 4, 8, 12}`), and the 131-134 replies carry that 12
  (`b2-protocol.md`). At gain 12 the ADC input full scale is plus or minus 0.2 V and one LSB is
  23.84 nV; with `sensorGain` 10 the logged `voltsPP` 0.04 is plus or minus 20 mV at the
  sensor, which is the `2 x 2.4 / (12 x 10)` the app prints. `sensorGain` is app-side
  (`b2-protocol.md`) and not read from the board; whether it models an analog stage on U50-U52 is
  open.
- **Reference choice on this board (inferred).** With a single Li-ion cell (3.0-4.2 V) behind
  LDOs, AVDD cannot reach the 4.4 V the 4 V reference needs, so 2.4 V is the only option here;
  AVDD was not measured.
- **Low bits.** The stream's third byte always ends `11` (`b2-protocol.md`). The ADS1294 sends all
  24 bits, so those two bits come from U57's firmware, not the converter (inferred).

## Data rate and clock against the 500 Hz stream

- The ADS129x `CONFIG1.DR[2:0]` table: with `HR` = 1 (high-resolution), fMOD = fCLK/4 and
  `DR` = 110 gives fMOD/1024 = **500 SPS**; with `HR` = 0 (low power), fMOD = fCLK/8 and
  `DR` = 101 gives fMOD/512 = 500 SPS. At fCLK = 2.048 MHz the ratios are exactly 500 SPS, so
  500 SPS is a native rate, not a decimation. The reset value `DR` = 110 with `HR` = 0 gives 250
  SPS, so firmware must set `HR` or `DR`.
- Z51 (2.048 MHz) is inside the datasheet's external-clock range (1.94-2.25 MHz, `CLKSEL` = 0);
  `CLKSEL` was not checked on the board, and the 2.048 MHz internal oscillator is the
  alternative (inferred that Z51 is used).
- The stream: 8 samples per 32-byte frame, 2 per channel, 250.0 frames/s exactly, so 500 samples/s
  per channel (observed, `b2-protocol.md`). That equals the native 500 SPS, so U57 forwards every
  conversion with no resampling (inferred). The 250.0 exactness follows from a crystal-derived
  fCLK.

## U60 (PIC12F1840)

- Pinout (PIC12(L)F1840 datasheet, 8-pin diagram, SOIC): 1 VDD, 2 RA5, 3 RA4, 4 RA3/MCLR/VPP,
  5 RA2, 6 RA1/ICSPCLK, 7 RA0/ICSPDAT, 8 VSS; 10-bit ADC (4 channels), EUSART, 2.3-5.5 V.
- It sits between `VBATA`/L53, J56, Q50 (with R570/R571 between the two), the push button and
  the side light pipe [`u60-q50-j56`, `u52-vbata-u60`]. A power-path controller (button sense,
  Q50 gate, battery voltage via its ADC) is inferred from that placement.
- **Unresolved:** which pins go to Q50, the button and J56 is not read; the photos do not
  separate the traces under the part. A meter on J56 and the Q50 gate settles it.

## Battery

- Label (observed, `battery-label`): `U463042PG 3.7V 600mAh 2.22Wh`, single-cell Li-ion polymer pouch,
  protection board under Kapton at one end, red and black leads to a white 2-pin plug.
- Size 4.6 x 30 x 42 mm from the code `463042` (inferred; the usual thickness/width/length
  coding, not measured).
- Connector: J59's pin pitch scales to about 2 mm against J58's 2.54 mm header, so JST PH 2.0
  (inferred from pitch; not measured).
- Polarity at J59 (observed, Operator report 2026-11-07): the pin nearer the WT12 module (Z50)
  is negative, the other pin positive.
- Battery field check: no field of the 110/112/131 replies reads as a battery voltage. A
  `VBATA/2` divider reads about 1.5-2.1 V for a 3.0-4.2 V cell: on a 12-bit ADC at 3.3 V that is
  1861-2606 counts, on the PIC's 10-bit ADC 465-653. The only unexplained payload is 112's
  `23 83` (0x2383 = 9091; bytes 35 and 131), outside both ranges, and it was identical in three
  handshakes taken at different times (`b2-protocol.md`). Result: not a battery voltage
  (inferred); 112 stays unresolved, and battery level may not be reported on this wire at all.

## Link budget and access points

- The B-2 streams 250 frames/s x 32 B = 8000 B/s. On a UART with 8N1 framing (10 bits per byte)
  that is 80 kbit/s, so the U57-to-Z50 UART needs more than 80 kbaud: 115200 is the lowest
  standard rate that carries it (0.69 load), and the 9600 8N1 the app opens is nominal for
  Bluetooth SPP (inferred; the Bluetooth link, not a physical UART, is what the app talks to).
  The WT12 UART runs 1200 baud to 3 Mbaud with RTS/CTS (WT12 datasheet, Table 14); the board
  exposes `RTS`/`CTS` pads. The RFCOMM link carries it within Bluetooth 2.1+EDR rates.
- The WT12 UART pins are TXD 26, RXD 6, NRTS 5, NCTS 10; its SPI is NCSB 21, SCLK 22, MISO 23,
  MOSI 24 (WT12 datasheet, pin descriptions). It also has USB D+/D- on pins 8/9; whether the
  board wires them is not observed.
- For hardware access (#68): SWD at J54 (unpopulated, needs a pogo or Tag-Connect cable); PIC
  programming at J56; BlueCore SPI at J61 (PSKey and firmware, debug use only per the WT12
  datasheet); ADS SPI pads (observe or inject the U54 bus); `MCO` (MCU clock output), `SDA2`/`SCL2`
  (a second I2C bus), `TESTSIG` (ADS test signal), the unpopulated `PB-1`, `PB-2`, `RESET`
  footprints, and the jumpers above.

## Safety

- The cell is Li-ion polymer: do not short, puncture, reverse or charge it outside a charger
  designed for one cell; a reversed J59 plug can destroy U60, Q50 or U61 (negative is the pin nearer Z50).
- The B-2 is an electrode-connected device. When any header (SWD, J52, ADS SPI pads) is wired to
  a PC, treat the board as non-isolated: do not connect a wearer's electrodes while a PC ground
  or a USB cable is attached to the board.
- Measure rails with a meter before attaching a debugger; `AGND` (J51) and `DGND` (J60) are
  separate taps whose link on the board is not traced.

## Unresolved

- J50's pin-to-net map (meter, see "Tracing").
- U60's pin use and role; J56 and J61 pinout; J52's function; what each jumper selects.
- The SMD codes `N20`, `B55Q`, `KE87`, `PKKP` (no exact marking-code match).
- Whether `sensorGain` 10 is an analog stage on this board.
- Which `CONFIG1`/`CONFIG3`/`CHnSET` values U57 programs (needs an SPI capture on the ADS pads).
