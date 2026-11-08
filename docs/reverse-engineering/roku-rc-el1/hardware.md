# Roku RC-EL1 hardware

Sources: owner's six board photos (2026-11), owner's multimeter readings, FCC
internal photos for TC2-R1028, Realtek UM0401 RTL872xD datasheet v3.4 (UM0401).
Tags: **observed** = read off the board or measured; **inferred** = from layout or
datasheet, not confirmed.

## Identity

- Remote RC-EL1, FCC ID TC2-R1028, board silkscreen RF342B V1.4 2019-09-18 (observed).
- Two-layer board (inferred: light passes through unpoured areas).
- Power: 2 × AAA in series (observed).

## Parts

| Ref | Marking | Role | Tag |
|---|---|---|---|
| U2 | RTL8721DF | Ameba-D Wi-Fi/BT SoC, firmware flash in package, 1.8 V I/O | marking observed; flash location inferred |
| Y1 | unreadable | 40 MHz crystal | inferred (UM0401 Table 3-6) |
| U4 | none, 6 pins | step-down regulator VBAT → 1V85, with L4 | inferred from layout |
| L4 | coil | U4 inductor | observed |
| F1 | `TH` | fuse on VBAT, next to TP9 | observed |
| Q1 | `Y1`, SOT-23 | NPN IR LED driver, likely SS8050 | inferred |
| D1 | — | IR LED, top edge | observed |
| U1 | none, 6 pads | unknown | observed pads; role unknown |
| U5 | — | MEMS microphone, button side | observed; digital (PDM) inferred |
| ANT1 | — | PCB antenna | observed (FCC photo) |
| SW1–SW23 | — | button pads, button side; SW7 sits over U2's ground-pad vias | observed |
| Pairing button | — | chip side, just below the battery bay | observed (owner); net unknown |

U2 pin 1 dot is at the bottom-right with the IR LED at the top (observed).

## P1 debug header

Pin names are observed from the silkscreen. Their mapping to U2 pins is inferred from
the names and UM0401, not traced.

| Pin | Name | Pin | Name |
|---|---|---|---|
| 1 | CHIP_EN | 2 | VBAT |
| 3 | RXD | 4 | GND |
| 5 | TXD | 6 | PWM7 |
| 7 | GND | 8 | DATA (SWD) |
| 9 | 1.85V | 10 | CLK (SWD) |

From UM0401 (inferred for this board):

- LOGUART TXD/RXD = PA[7]/PA[8] (pin diagrams, §2.1 Package Types).
- PA[7] is the `UART_DOWNLOAD` trap pin: low at power-on enters ROM flash download
  (Table 2-4, §2.4).
- SWD sits on PA[27] DATA / PB[3] CLK while logical eFuse 0x0e bit 0 is 1 (default),
  or on PB[19] DATA / PB[18] CLK when that bit is programmed to 0 (Table 2-3 notes 1–2).
  Which pair P1.8/P1.10 reach is unknown.
- PWM7 is labelled on PA[30] in the §2.1 pin diagrams. PA[30] is the `SPS_SEL` trap pin
  (1 = SPS, 0 = LDO for the internal 1.1 V regulator, Table 2-4). Do not drive P1.6 low at reset.

P1 pads are convex solder bumps; probing them while holding a second probe is impractical.

## Measured nets (batteries in)

- VBAT 2.88 V: P1.2 (vs GND); through-hole 5 has continuity to P1.2. TP9 reads 2.88 V,
  same net inferred from voltage only.
- GND: P1.4, P1.7; through-hole 4 has continuity to GND.
- 1V85: P1.9 1.85 V (vs GND), TP10 1.84 V, TP11 1.84 V. TP10–TP11 continuity confirmed;
  P1.9 on the same rail inferred from voltage only.
- P1.5 TXD idles at 1.85 V (vs GND).
- TP1, TP2, TP3, TP5 ≈ 1.84 V, no continuity to the 1V85 rail → signals held high.
- TP4, TP6, TP7, TP8 ≈ 0 V, no continuity to GND → signals held low.
- Through-holes 1–3: no continuity to any P1 pin.

TP9 and TP1–TP8 were read against TP11 and offset by +1.84 V; the other readings are vs GND.

## Partial schematic

`?` marks an inferred, unmeasured link or part. R2 and L2 are guessed references from
the photos.

```
BT1 2xAAA (2.88V) ─ VBAT ─┬─ F1 ─ U4 + L4 (buck?) ─ 1V85 ─┬─ U2 RTL8721DF supply
                          ├─ P1.2, hole 5, TP9?           ├─ TP10, TP11, P1.9?
                          └─ D1 anode? ─ D1 ─ R2? ─ Q1 (NPN?) ─ GND
U2 XI/XO ─ Y1 40 MHz?          U2 RF ─ match (L2?) ─ ANT1
U2 PA7 (TXD)? ─ P1.5   U2 PA8 (RXD)? ─ P1.3   U2 CHIP_EN? ─ P1.1
U2 SWD (PA27/PB3 or PB19/PB18)? ─ P1.8 DATA, P1.10 CLK    U2 PA30 (PWM7)? ─ P1.6
U2 GPIO? ─ keypad matrix SW1–SW23 (unmapped), pairing button (unmapped), U5 mic?
GND ─ P1.4, P1.7, hole 4
```

## Firmware dump plan

Inferred from UM0401; not attempted.

1. Get a 1.8 V-capable USB-serial adapter, or a 3.3 V adapter through a level shifter.
2. Connect GND to P1.4, adapter RX to P1.5 TXD, adapter TX to P1.3 RXD (pogo clip or fine wire).
3. Hold P1.5 low, pulse P1.1 CHIP_EN low, then release P1.5 once CHIP_EN is high again:
   PA[7] is sampled only at power-on, and it must be free to transmit afterwards.
4. Read flash with Realtek's Ameba-D image tool or an open-source equivalent.

## Open items

- Which chip pins TP1–TP8 and through-holes 1–3 reach.
- Cross-side continuity: P1 TXD/RXD/CHIP_EN vs TP1/2/3/5; P1 CLK/DATA/PWM7 vs TP4/6/7/8.
- P1.9 vs TP10 and TP9 vs P1.2 continuity.
- U1 function; U4 and Y1 part numbers.
- Keypad matrix and pairing button GPIOs (UM0401 lists KEY_ROW/KEY_COL keyscan pins).
