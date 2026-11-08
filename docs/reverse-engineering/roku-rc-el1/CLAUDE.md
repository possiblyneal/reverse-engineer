## Purpose

Durable findings from hardware analysis of the owner's Roku **RC-EL1** voice remote
(FCC ID TC2-R1028, board RF342B V1.4). Goal: dump and patch its firmware so the
Disney+ button opens YouTube.

## Ownership

| Path | What |
|---|---|
| `hardware.md` | The board from the owner's photos, multimeter readings, the FCC internal photos and Realtek UM0401: parts list with observed-vs-inferred tags, P1 debug-header pinout, measured nets (VBAT, 1V85, GND, test points), partial schematic, firmware-dump plan via the RTL8721D UART download mode, and the open items. Photos stay outside Git. |

## Local Contracts

Inherits every contract in the parent `../CLAUDE.md`.

- The owner probes alone with one hand: no checks that need a second person, a wire
  through a hole, or new solder joints. Cross-side continuity waits until P1 is wired.
- Chip I/O is 1.8 V. Never connect a 3.3 V or 5 V serial adapter to P1 directly.

## Work Guidance

## Verification

## Child Index
