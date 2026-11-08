# Roku RC-EL1 hardware

Sources: owner's six board photos (2026-11), owner's multimeter readings, FCC
internal photos for TC2-R1028, Realtek UM0401 RTL872xD datasheet v3.4.
Tags: **observed** = read off the board or measured; **inferred** = from layout or
datasheet, not confirmed.

## Identity

- Remote RC-EL1, FCC ID TC2-R1028, board silkscreen RF342B V1.4 2019-09-18 (observed).
- Two-layer board (inferred: light passes through unpoured areas).
- Power: 2 × AAA in series (observed).

## Parts

| Ref | Marking | Role | Tag |
|---|---|---|---|
| U2 | RTL8721DF | Ameba-D Wi-Fi/BT SoC, firmware flash in package, 1.8 V I/O | observed marking; flash location inferred |
| Y1 | unreadable | 40 MHz crystal | inferred from UM0401 |
| U4 | none, 6 pins | step-down regulator VBAT → 1V85, with L4 | inferred from layout |
| L4 | coil | U4 inductor | observed |
| F1 | `TH` | fuse on VBAT, next to TP9 | observed |
| Q1 | `Y1`, SOT-23 | NPN IR LED driver, likely SS8050 | inferred |
| D1 | — | IR LED, top edge | observed |
| U1 | none, 6 pads | unknown | — |
| U5 | — | MEMS microphone, button side | observed; digital (PDM) inferred |
| ANT1 | — | PCB antenna | observed (FCC photo) |
| SW1–SW23 | — | button pads, button side; SW7 sits over U2's ground-pad vias | observed |
| Pairing button | — | chip side, just below the battery bay | observed (owner); net unknown |

U2 pin 1 dot is at the bottom-right with the IR LED at the top (observed).

## P1 debug header (silkscreen, observed)

| Pin | Name | Pin | Name |
|---|---|---|---|
| 1 | CHIP_EN | 2 | VBAT |
| 3 | RXD | 4 | GND |
| 5 | TXD | 6 | PWM7 |
| 7 | GND | 8 | DATA (SWD) |
| 9 | 1.85V | 10 | CLK (SWD) |

Per UM0401, LOGUART is PA[7]/PA[8], and PA[7] (TXD) held low at reset enters the
ROM UART download mode. SWD is on PA[27]/PB[3] only if the eFuse enables it.
P1 pads are convex solder bumps; probing them while holding a second probe is impractical.

## Measured nets (batteries in)

- VBAT 2.88 V: P1.2, TP9, through-hole 5.
- GND: P1.4, P1.7, through-hole 4.
- 1V85 1.84–1.85 V: P1.9, TP10, TP11 (TP11–TP10 continuity confirmed).
- P1.5 TXD idles at 1.85 V.
- TP1, TP2, TP3, TP5 ≈ 1.84 V and not on the 1V85 rail → signals held high.
- TP4, TP6, TP7, TP8 ≈ 0 V, no continuity to GND → signals held low.
- Through-holes 1–3: no continuity to any P1 pin.

Readings were taken against TP11 as reference and offset by +1.84 V.

## Partial schematic

```
BT1 2xAAA (2.88V) ─ VBAT ─┬─ F1 ─ U4 + L4 (buck?) ─ 1V85 ─┬─ U2 RTL8721DF supply
                          ├─ P1.2, TP9, hole 5            ├─ P1.9, TP10, TP11
                          └─ D1 anode? ─ D1 ─ R2? ─ Q1 (NPN?) ─ GND
U2 XI/XO ─ Y1 40 MHz?          U2 RF ─ match (L2?) ─ ANT1
U2 PA7 (TXD) ─ P1.5   U2 PA8 (RXD) ─ P1.3   U2 CHIP_EN ─ P1.1
U2 SWD ─ P1.8 DATA, P1.10 CLK    U2 PWM7? ─ P1.6
U2 GPIO ─ keypad matrix SW1–SW23 (unmapped), pairing button (unmapped), U5 mic?
GND ─ P1.4, P1.7, hole 4
```

## Firmware dump plan

1. Get a 1.8 V-capable USB-serial adapter, or a 3.3 V adapter through a level shifter.
2. Connect GND to P1.4, adapter RX to P1.5 TXD, adapter TX to P1.3 RXD (pogo clip or fine wire).
3. Hold P1.5 low and pulse P1.1 CHIP_EN low to enter UART download mode.
4. Read flash with Realtek's Ameba-D image tool or an open-source equivalent.
Not attempted yet.

## Open items

- Which chip pins TP1–TP8 and through-holes 1–3 reach.
- Cross-side continuity: P1 TXD/RXD/CHIP_EN vs TP1/2/3/5; P1 CLK/DATA/PWM7 vs TP4/6/7/8.
- U1 function; U4 and Y1 part numbers.
- Keypad matrix and pairing button GPIOs.
