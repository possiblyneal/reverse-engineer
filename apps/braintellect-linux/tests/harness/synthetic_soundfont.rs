//! A synthetic SoundFont 2 file: invented looped sine samples in the SF2 chunk
//! layout (`sdta/smpl`, then `pdta` with the nine records the clone reads).
//! It carries no vendor or third-party sample bytes.

/// One instrument zone: a looped sine sample over a key range.
pub struct Zone {
    pub keys: (u8, u8),
    pub rate_hz: u32,
    /// Samples per sine period; the sample holds a whole number of periods.
    pub period: u32,
    /// The sample header's original pitch (MIDI key).
    pub header_pitch: u8,
    /// `overridingRootKey` generator, when the zone sets one.
    pub root_override: Option<u8>,
    /// `fineTune` generator in cents, when the zone sets one.
    pub fine_cents: Option<i16>,
    /// The zone loops the sample with `sampleModes`; otherwise the instrument's
    /// global zone must.
    pub loops: bool,
}

impl Zone {
    /// The pitch the sample really has, Hz.
    pub fn sample_hz(&self) -> f64 {
        f64::from(self.rate_hz) / f64::from(self.period)
    }
}

/// One preset (bank 0) with one instrument.
pub struct Preset {
    pub program: u16,
    /// The instrument's global zone sets `sampleModes` to loop.
    pub global_loop: bool,
    pub zones: Vec<Zone>,
}

const PERIODS: u32 = 20;

fn sine(zone: &Zone) -> Vec<i16> {
    (0..zone.period * PERIODS)
        .map(|i| {
            let phase = f64::from(i) / f64::from(zone.period) * std::f64::consts::TAU;
            (phase.sin() * 12000.0).round() as i16
        })
        .collect()
}

fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    out.extend(u32::try_from(body.len()).expect("size").to_le_bytes());
    out.extend(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
    out
}

fn list(kind: &[u8; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut body = kind.to_vec();
    for c in chunks {
        body.extend(c);
    }
    chunk(b"LIST", &body)
}

fn name20(name: &str) -> [u8; 20] {
    let mut out = [0u8; 20];
    out[..name.len()].copy_from_slice(name.as_bytes());
    out
}

fn gen(op: u16, amount: u16) -> [u8; 4] {
    let mut out = [0u8; 4];
    out[..2].copy_from_slice(&op.to_le_bytes());
    out[2..].copy_from_slice(&amount.to_le_bytes());
    out
}

fn u16le(v: usize) -> [u8; 2] {
    u16::try_from(v).expect("index").to_le_bytes()
}

fn u32le(v: usize) -> [u8; 4] {
    u32::try_from(v).expect("index").to_le_bytes()
}

/// The bytes of a SoundFont holding `presets`.
pub fn soundfont(presets: &[Preset]) -> Vec<u8> {
    soundfont_after_unused_samples(presets, 0)
}

/// Like [`soundfont`], with `unused` empty sample headers ahead of the presets'
/// samples, so their `sampleID`s start at `unused`.
pub fn soundfont_after_unused_samples(presets: &[Preset], unused: usize) -> Vec<u8> {
    let (mut smpl, mut shdr) = (Vec::new(), Vec::new());
    let (mut phdr, mut pbag, mut pgen) = (Vec::new(), Vec::new(), Vec::new());
    let (mut inst, mut ibag, mut igen) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..unused {
        shdr.extend(name20("unused"));
        shdr.extend([0u8; 26]);
    }
    let mut sample_count = unused;
    for (n, preset) in presets.iter().enumerate() {
        phdr.extend(name20("Synthetic"));
        phdr.extend(preset.program.to_le_bytes());
        phdr.extend(0u16.to_le_bytes());
        phdr.extend(u16le(n));
        phdr.extend([0u8; 12]);
        pbag.extend(u16le(pgen.len() / 4));
        pbag.extend(0u16.to_le_bytes());
        pgen.extend(gen(41, u16::try_from(n).expect("instrument")));

        inst.extend(name20("Synthetic"));
        inst.extend(u16le(ibag.len() / 4));
        if preset.global_loop {
            ibag.extend(u16le(igen.len() / 4));
            ibag.extend(0u16.to_le_bytes());
            igen.extend(gen(54, 1));
        }
        for zone in &preset.zones {
            ibag.extend(u16le(igen.len() / 4));
            ibag.extend(0u16.to_le_bytes());
            igen.extend(gen(
                43,
                u16::from(zone.keys.0) | u16::from(zone.keys.1) << 8,
            ));
            if let Some(cents) = zone.fine_cents {
                igen.extend(gen(52, cents as u16));
            }
            if zone.loops {
                igen.extend(gen(54, 1));
            }
            if let Some(root) = zone.root_override {
                igen.extend(gen(58, u16::from(root)));
            }
            igen.extend(gen(53, u16::try_from(sample_count).expect("sample")));
            let pcm = sine(zone);
            let start = smpl.len() / 2;
            // SF2 pads each sample with 46 zero points.
            smpl.extend(pcm.iter().flat_map(|s| s.to_le_bytes()));
            smpl.extend([0u8; 92]);
            shdr.extend(name20("sine"));
            for v in [start, start + pcm.len(), start, start + pcm.len()] {
                shdr.extend(u32le(v));
            }
            shdr.extend(zone.rate_hz.to_le_bytes());
            shdr.push(zone.header_pitch);
            shdr.push(0);
            shdr.extend(0u16.to_le_bytes());
            shdr.extend(1u16.to_le_bytes());
            sample_count += 1;
        }
    }
    // Terminal records.
    phdr.extend(name20("EOP"));
    phdr.extend([0u8; 4]);
    phdr.extend(u16le(presets.len()));
    phdr.extend([0u8; 12]);
    pbag.extend(u16le(pgen.len() / 4));
    pbag.extend(0u16.to_le_bytes());
    pgen.extend(gen(0, 0));
    inst.extend(name20("EOI"));
    inst.extend(u16le(ibag.len() / 4));
    ibag.extend(u16le(igen.len() / 4));
    ibag.extend(0u16.to_le_bytes());
    igen.extend(gen(0, 0));
    shdr.extend(name20("EOS"));
    shdr.extend([0u8; 26]);

    let sdta = list(b"sdta", &[chunk(b"smpl", &smpl)]);
    let pdta = list(
        b"pdta",
        &[
            chunk(b"phdr", &phdr),
            chunk(b"pbag", &pbag),
            chunk(b"pmod", &[0u8; 10]),
            chunk(b"pgen", &pgen),
            chunk(b"inst", &inst),
            chunk(b"ibag", &ibag),
            chunk(b"imod", &[0u8; 10]),
            chunk(b"igen", &igen),
            chunk(b"shdr", &shdr),
        ],
    );
    let info = list(b"INFO", &[chunk(b"ifil", &[2, 0, 1, 0])]);
    let mut body = b"sfbk".to_vec();
    for part in [info, sdta, pdta] {
        body.extend(part);
    }
    chunk(b"RIFF", &body)
}
