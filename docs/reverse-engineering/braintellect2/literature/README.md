# BrainState / HIRREM / Cereset — Research Library

Peer-reviewed literature on the technology inside the BRAINtellect device
(Brain State Technologies; Wake Forest / Atrium Health research program).
The device delivers **HIRREM** — High-resolution, Relational, Resonance-based,
Electroencephalic Mirroring (later commercialized as **Cereset / Cereset
Research**): 2-channel scalp EEG → the dominant frequency in each short time
window is mapped by proprietary math to **audible musical tones** played back in
near-real-time, so the tone *mirrors/resonates* with current brain activity
(not threshold-and-reward neurofeedback).

Collected 2026-10-16. `.md` files are full article text (saved because the
publisher sites are Cloudflare/reCAPTCHA-gated against direct PDF download);
`.pdf` files are the original open-access PDFs.

Three originals exceed the repo's 500 KB commit guard and are therefore **not
committed** — reach them by the DOI/PMC links below (local copies, if fetched,
live out of Git under `tmp/literature-oversized/`): the 2018 Shaltout PDF, the
2015 PTSD study protocol PDF, and the 1-min tone `AudioSample` in the
supplement.

## Core mechanism / methodology
- **2013_Gerdes-Tegeler_HIRREM-allostatic-methodology_BrainBehav.md** — *the*
  foundational paper describing how HIRREM works (tone generation from dominant
  frequency; allostatic auto-calibration). Gerdes L, Gerdes P, Lee SW, Tegeler CH.
  *Brain Behav* 2013;3(2):193–205. doi:10.1002/brb3.116 · PMC3607159

## Insomnia (the condition with the most detailed protocol)
- **2012_Tegeler_HIRREM-insomnia-crossover-pilot_BrainBehav.md** — open-label
  randomized crossover pilot. Tegeler CH et al. *Brain Behav* 2012. PMC3500468
- **2020_Tegeler_HIRREM-insomnia-RCT_BrainBehav.md** — randomized,
  placebo-controlled trial (symptoms + autonomic function). Tegeler CL et al.
  *Brain Behav* 2020;10(11):e01826. doi:10.1002/brb3.1826 · PMC7667311
- **2023_Tegeler_Cereset-Research-SOP-insomnia-RCT_GAIMH.md** — **modern device
  standard operating procedures** (sensor sites + protocol sequence for the
  current Cereset Research hardware); randomized controlled trial. Tegeler CL,
  Munger Clary H, Shaltout HA, Simpson SL, Gerdes L, Tegeler CH. *Glob Adv Integr
  Med Health* 2023;12:27536130221147475. doi:10.1177/27536130221147475 · PMC9933987

## Autonomic / cardiovascular
- **2018_Shaltout_HRV-baroreflex-sleep-allostatic_FrontPublicHealth** *(not
  committed — over 500 KB; open-access at the link)* — heart rate variability,
  baroreflex sensitivity, sleep (heterogeneous cohort). Shaltout HA, Lee SW,
  et al. *Front Public Health* 2018;6:116.
  doi:10.3389/fpubh.2018.00116 · PMC5996903

## PTSD (military)
- **2017_Tegeler_PTSD-military-closed-loop_MilMedRes.md** — self-reported and
  autonomic improvements. Tegeler CL et al. *Mil Med Res* 2017;4:38.
  doi:10.1186/s40779-017-0147-0 · PMC5740870
- **2015_PROTOCOL_HIRREM-PTSD-military_NCT03230890** *(not committed — over
  500 KB; see ClinicalTrials.gov NCT03230890)* — the IRB-approved study
  protocol + statistical analysis plan for the PTSD trial (NCT03230890). Useful
  because it spells out the as-run clinical procedure in detail.

## Concussion / TBI
- **2016_Tegeler_post-concussion-athletes_SportsMedOpen.md** — athletes with
  persisting post-concussion symptoms (preliminary). Tegeler CH et al. *Sports
  Med Open* 2016;2:39. doi:10.1186/s40798-016-0063-y · PMC5023638
- **2023_Cole-Tegeler_postconcussive-RCT_AnnClinTranslNeurol.md** — randomized
  controlled trial, acoustic stimulation for postconcussive symptoms. Cole WR,
  Tegeler CL, et al. *Ann Clin Transl Neurol* 2024;11(1):105–120.
  doi:10.1002/acn3.51937 · PMC10791035

## Related (NOT BrainState — different groups, for context only)
- **RELATED_2022_biorxiv_closed-loop-auditory-MEG-sleep_abstract.md** — MEG study
  of closed-loop auditory stimulation in sleep. doi:10.1101/2022.12.19.521103

## Not saved here (abstracts, registrations, or paywalled) — references
- Neurology 2020;94(15 suppl):4404 — pilot placebo-controlled Cereset Research, insomnia. doi:10.1212/wnl.94.15_supplement.4404
- SLEEP 2020 abstract 0523 — closed-loop AI acoustic neuromodulation, insomnia. doi:10.1093/sleep/zsaa056.520
- SLEEP 2017 abstract. doi:10.1093/sleepj/zsx050.388
- Neurology 2021;96(15 suppl):4298. doi:10.1212/wnl.96.15_supplement.4298
- Brain Injury 2025 — acoustic neuromodulation ± micro-voltage tACS, postconcussive (Taylor & Francis, paywalled). doi:10.1080/02699052.2024.2445709 · PMID 39913294
- ClinicalTrials.gov: NCT01971567 (insomnia), NCT03607994 (HIRREM-SOP insomnia), NCT03230890 (PTSD), NCT03649958 (TBI, sponsor Brain State Technologies)
- Program pages: Wake Forest HIRREM research — school.wakehealth.edu/departments/neurology/hirrem/research ; Cereset Research publications — school.wakehealth.edu/departments/neurology/brain-body-research-program/cereset-research/publications

## Relevance to the device repair
The 2013 mechanism paper + the 2023 Cereset Research SOP are the two that
describe what the running software actually does (dominant-frequency→tone
mapping) and the montage/sensor placement a session uses. They are background
for understanding the recovered `flowDesign`/montage/placement data, not a
substitute for the real design blobs — the device plays Brain State's specific
proprietary algorithm, which these papers describe but do not fully specify.
