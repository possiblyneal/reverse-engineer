# Related Reading — EEG→Audio Feedback (candidates to verify)

Leads found via web search on 2026-11-03 for understanding and improving the
BRAINtellect/HIRREM EEG→tone feedback. **Not downloaded**; these are external
publications to read at the source. Verify each before relying on it. The
in-repo corpus (`README.md`) is the Brain State / Tegeler program itself; this
list is independent work that explains the *mechanism class* or offers
*engineering improvements*.

Why this matters here: the device maps each short-window dominant EEG frequency
to an audible tone in near-real-time (`README.md`; `b2-protocol.md`
for the wire side; the type-9 cleartext series in `session-storage.md`
is the computed feedback the clone must reproduce). The items below are the
independent literature on that exact operation and on doing it better.

## A. EEG → audio mapping (the core mechanism, independent of Brain State)

- **Wu, Li, Yao et al. — "Scale-Free Music of the Brain." PLOS One 2009.**
  doi:10.1371/journal.pone.0005915 — the canonical independent method for
  translating scalp EEG into musical notes (period→pitch, amplitude→intensity).
  Closest non-proprietary analog to HIRREM's dominant-frequency→tone step.
- **Wu et al. — "Scale-Free Brain Quartet: Artistic Filtering of Multi-Channel
  Brainwave Music." PLOS One 2013.** doi:10.1371/journal.pone.0064046 —
  multi-channel extension; relevant to the B-2's 2–4 channel montage.
- **Deuel et al. — "The Encephalophone: A Novel Musical Biofeedback Device using
  Conscious Control of EEG." Front Hum Neurosci 2017.**
  doi:10.3389/fnhum.2017.00213 — EEG posterior-dominant-rhythm → tone biofeedback;
  concrete feature→pitch pipeline and latency discussion.
- **Steffert, T. — "Real-Time EEG Sonification for Neurofeedback" (PhD thesis,
  Open University, 2017).** oro.open.ac.uk/57965 — book-length treatment of
  real-time sonification design choices; the most thorough single reference.
- **Väljamäe, Steffert, Holland et al. — "A Review of Real-Time EEG Sonification
  Research." ICAD 2013.** oro.open.ac.uk/38478 — the survey; taxonomy of mapping
  strategies, good for situating HIRREM's approach.
- **Eaton & Miranda — "On Mapping EEG Information into Music" (in *Guide to
  Brain-Computer Music Interfacing*, 2018).** Plymouth PEARL — mapping-strategy
  catalogue (direct, generative, model-based).

## B. Closed-loop auditory stimulation — engineering to improve the feedback

- **Esfahani et al. — "Closed-loop auditory stimulation of sleep slow
  oscillations: Basic principles and best practices." Neurosci Biobehav Rev
  2023.** doi:10.1016/j.neubiorev.2023.105379 — the best-practices review;
  real-time detection, phase targeting, pitfalls. Primary improvement reference.
- **"CLAS-hdEEG: high-density EEG platform for real-time delta wave detection and
  closed-loop auditory stimulation." J Neural Eng 2026.**
  doi:10.1088/1741-2552/ae4e5b — open software architecture for real-time
  detect→stimulate; a design template for the clone's feedback loop.
- **Hebron et al. — "A closed-loop auditory stimulation approach selectively
  modulates alpha oscillations and sleep onset dynamics in humans." 2023.**
  PMC11185466 — alpha-band targeting (HIRREM targets relaxation/alpha), not just
  sleep SOs.
- **Harlow et al. — "Individualized Closed-Loop Acoustic Stimulation Suggests an
  Alpha Phase Dependence of Sound-Evoked and Induced Brain Activity." 2024.**
  PMC11181104 — phase-dependence of auditory response; argues for phase-aware
  tone timing.
- **"Acoustic stimulation and other emerging approaches to enhance sleep: design
  notes for the next generation of closed-loop neurostimulation technology."
  Front Neurosci 2026.** doi:10.3389/fnins.2025.1682450 — forward-looking design
  constraints.

## C. Autonomic / temporal-asymmetry mechanism (independent of Tegeler group)

- **Fortunato et al. — "Use of an allostatic neurotechnology by adolescents with
  POTS … improvements in HRV and changes in temporal lobe electrical activity."
  Exp Brain Res 2015.** PMC4751215 — independent (VCU) replication of the
  autonomic + temporal-asymmetry signal the Brain State papers report.

## D. Efficacy caveats & trial design (so parity tests stay honest)

- **Thibault & Raz — "The psychology of neurofeedback: Clinical intervention even
  if applied placebo." American Psychologist 2017.** doi:10.1037/amp0000118 — and
  "Neurofeedback or neuroplacebo?" *Brain* 2017;140(4):862. The placebo critique.
- **Parsons — "Rethinking control conditions in clinical neurofeedback trials."
  Discover Neuroscience 2026.** doi:10.1186/s13064-026-00252-x — control-condition
  design.
- **Ros et al. — "The Do's and Don'ts of Neurofeedback Training: A Review of the
  Controlled Studies Using Healthy Adults." Front Hum Neurosci 2016.**
  doi:10.3389/fnhum.2016.00301 — the checklist lineage (→ CRED-nf) for sound
  neurofeedback study design.

## Not pursued (adjacent but off-target)
Generative/artistic BCMI performance systems (MaxMuse, Csound's Brain, Reflex-in,
SSVEP music BCIs) translate EEG to music for *expression*, not clinical
mirroring; listed only to note they were screened and set aside.
