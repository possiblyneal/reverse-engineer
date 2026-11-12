//! The `B-2-Circle-Balance-fromBAGSD-v5.1.1` design: the B-2 Balance Meter. It is a report
//! design (`designRole` `BAGSD.report`), not a live one: the original runs it after a session
//! over the session's stored `LMH` series, never over the EEG stream.
//!
//! Each of its eight chart inputs is two chains, `BAGSDataTypeBandFilter` (one LMH band of one
//! channel) -> `TimeFilter` -> the `absolute` `AggregatorFilter`, paired by an
//! `ExpressionTransform` into one Balance value. Evidence and open items:
//! `docs/reverse-engineering/braintellect2/audio-feedback.md`, "Circle-Balance design".

/// The LMH band a chart input reads (`BAGSDataTypeBand`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LmhBand {
    High,
    Low,
}

/// The `CircleGraphChart` element an input is drawn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meter {
    /// `Emotional State`: SELF / FEELINGS ABOUT / OTHERS, channels 1 and 2.
    Emotional,
    /// `Activation State`: BRAKE / STUCK ON THE / GAS, channels 3 and 4.
    Activation,
}

/// The chart's graph (`Chart1` titled `Before`, `Chart2` titled `After`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Graph {
    Before,
    After,
}

/// The input's `rangeAxis`: `top` is labelled OUTWARD, `bottom` INWARD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Top,
    Bottom,
}

/// `exerciseTimeAnchorPoint` of a `TimeFilter`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    End,
}

/// A `TimeFilter` (vtable 0x18bb0ac): `begin_s` and `end_s` count from the exercise start, or
/// back from its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeWindow {
    pub anchor: Anchor,
    pub begin_s: u32,
    pub end_s: u32,
}

impl TimeWindow {
    /// The kept range `(lo, hi]` in milliseconds for an exercise of `duration_s` (the type-2
    /// start message, receive slot 0x624a90): `begin * 1000, end * 1000` from the start, or
    /// `(duration - begin) * 1000, (duration - end) * 1000` from the end.
    #[must_use]
    pub fn bounds_ms(self, duration_s: u32) -> (i64, i64) {
        let (begin, end, duration) = (
            i64::from(self.begin_s),
            i64::from(self.end_s),
            i64::from(duration_s),
        );
        match self.anchor {
            Anchor::Start => (begin * 1000, end * 1000),
            Anchor::End => ((duration - begin) * 1000, (duration - end) * 1000),
        }
    }

    /// Whether a series point at `x_ms` passes (0x624a90, type 8: strict lower bound,
    /// inclusive upper bound).
    #[must_use]
    pub fn keeps(self, x_ms: i64, duration_s: u32) -> bool {
        let (lo, hi) = self.bounds_ms(duration_s);
        lo < x_ms && x_ms <= hi
    }
}

/// One `CircleGraphChart` input: the band, window and channel pair behind it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeterInput {
    pub meter: Meter,
    pub graph: Graph,
    pub axis: Axis,
    pub band: LmhBand,
    pub window: TimeWindow,
    /// The channels (1-based) feeding the expression's `A` and `B`.
    pub channels: (usize, usize),
    /// The `ExpressionTransform` text; [`balance_meter`] is its evaluation.
    pub expression: String,
}

/// The design's chart inputs, in the order `make-fixtures.py` writes them (meter, graph,
/// axis).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CircleBalanceDesign {
    pub inputs: Vec<MeterInput>,
}

/// `graphMaxRangeValue` of every `CircleGraphChart` graph: the chart's full scale, the value it
/// clips its drawing to (`audio-feedback.md`, "Circle-Balance design").
pub const GRAPH_MAX_RANGE: f32 = 4.0;

/// The text of all eight `ExpressionTransform`s.
pub const BALANCE_EXPRESSION: &str =
    "ln(1+abs((((1.08*B)-A)*10)/(A min (B*1.08))))*sign((1.08*B)-A)";

/// One stored LMH point: `xValue` in milliseconds and `yValue` as the original's `float32`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LmhPoint {
    pub x_ms: i64,
    pub y: f32,
}

/// A stored exercise's `High` and `Low` LMH series, Channel1..4 (`exerciseDataChannel`
/// `dataType="LMH"`).
#[derive(Clone, Debug, Default)]
pub struct Lmh {
    high: [Vec<LmhPoint>; 4],
    low: [Vec<LmhPoint>; 4],
}

impl Lmh {
    /// The series of `channel` (1-based) in `band`.
    #[must_use]
    pub fn series(&self, channel: usize, band: LmhBand) -> &[LmhPoint] {
        match band {
            LmhBand::High => &self.high[channel - 1],
            LmhBand::Low => &self.low[channel - 1],
        }
    }

    /// The series of `channel` (1-based) in `band`, to fill.
    pub fn series_mut(&mut self, channel: usize, band: LmhBand) -> &mut Vec<LmhPoint> {
        match band {
            LmhBand::High => &mut self.high[channel - 1],
            LmhBand::Low => &mut self.low[channel - 1],
        }
    }
}

/// The `absolute` Aggregator over the points a window keeps: a double running sum, then
/// sum / count rounded to `float32` (receive 0x649cc0, accumulator vtable 0x18bf978, mean
/// 0x649850; bit-exact against the original's chart inputs). `None` when
/// the window keeps no point, as the aggregator then has nothing to emit.
#[must_use]
pub fn window_mean(points: &[LmhPoint], window: TimeWindow, duration_s: u32) -> Option<f32> {
    let (sum, count) = points
        .iter()
        .filter(|p| window.keeps(p.x_ms, duration_s))
        .fold((0.0_f64, 0_u32), |(sum, count), p| {
            (sum + f64::from(p.y), count + 1)
        });
    (count > 0).then(|| (sum / f64::from(count)) as f32)
}

/// [`BALANCE_EXPRESSION`] over the two window means: evaluated in double precision (muParser,
/// RTTI `mu::ParserError`), rounded to `float32`, and 0 when not finite (0x63b120).
#[must_use]
pub fn balance_meter(a: f32, b: f32) -> f32 {
    let (a, b) = (f64::from(a), f64::from(b));
    let gained = 1.08 * b - a;
    let value = (1.0 + ((gained * 10.0) / a.min(b * 1.08)).abs()).ln() * sign(gained);
    if value.is_finite() {
        value as f32
    } else {
        0.0
    }
}

/// muParser's `sign`: -1, 0 or 1.
fn sign(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

impl CircleBalanceDesign {
    /// The design as decoded from the original's design XML.
    #[must_use]
    pub fn circle_balance() -> Self {
        let before = TimeWindow {
            anchor: Anchor::Start,
            begin_s: 60,
            end_s: 120,
        };
        let after = TimeWindow {
            anchor: Anchor::End,
            begin_s: 120,
            end_s: 60,
        };
        let mut inputs = Vec::new();
        for (meter, channels) in [(Meter::Activation, (3, 4)), (Meter::Emotional, (1, 2))] {
            for (graph, window) in [(Graph::Before, before), (Graph::After, after)] {
                for (axis, band) in [(Axis::Bottom, LmhBand::Low), (Axis::Top, LmhBand::High)] {
                    inputs.push(MeterInput {
                        meter,
                        graph,
                        axis,
                        band,
                        window,
                        channels,
                        expression: BALANCE_EXPRESSION.to_owned(),
                    });
                }
            }
        }
        Self { inputs }
    }

    /// Each input's chart value over a stored exercise of `duration_s`, in input order:
    /// [`balance_meter`] of the `A` and `B` channels' [`window_mean`]s, `None` when a window
    /// keeps no point.
    #[must_use]
    pub fn report(&self, lmh: &Lmh, duration_s: u32) -> Vec<Option<f32>> {
        self.inputs
            .iter()
            .map(|input| {
                let mean = |channel| {
                    window_mean(lmh.series(channel, input.band), input.window, duration_s)
                };
                Some(balance_meter(
                    mean(input.channels.0)?,
                    mean(input.channels.1)?,
                ))
            })
            .collect()
    }
}
