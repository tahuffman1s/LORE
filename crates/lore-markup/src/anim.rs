//! Transitions, keyframe animations and timing functions.
//!
//! Time is a whole number of ticks. Progress is worked out in `f64` from the
//! tick count and the tick rate, so the same ticks always give the same
//! picture. Animated values are written into the screen's `used` styles;
//! the cascade's own computed styles are never touched.
//!
//! Layering, bottom to top: the computed style, transitions (they ease a
//! change of computed value), then keyframe animations (later in the list
//! wins). A property that changes while its transition runs restarts from
//! the value on screen; changing it back to where it began is just another
//! restart, not a shortened reversal.

use crate::css::PropId;
use crate::css::parse::{DeclValue, Declaration, KeyframeStop};
use crate::css::value::{
    AnimDirection, FillMode, IterationCount, PlayState, StepPos, Timing, TransProp, Value,
};
use crate::dom::{Document, ElementId};
use crate::error::MarkupError;
use crate::style::{AnimValue, Animation, ComputedStyle, Effect, StyleChange, Styles, effect_of};

// ----- timing functions ------------------------------------------------------

/// Applies a timing function to progress `x` (clamped to 0..=1).
pub fn ease(timing: Timing, x: f64) -> f64 {
    let x = if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) };
    match timing {
        Timing::Linear => x,
        Timing::Cubic(p) => cubic_bezier(
            f64::from(p[0]),
            f64::from(p[1]),
            f64::from(p[2]),
            f64::from(p[3]),
            x,
        ),
        Timing::Steps(n, pos) => steps(n, pos, x),
    }
}

/// `cubic-bezier(x1, y1, x2, y2)` at `x`: Newton's method, then bisection
/// when the slope is too flat. Both loops have fixed bounds.
fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let coeffs = |a: f64, b: f64| {
        let c = 3.0 * a;
        let bb = 3.0 * (b - a) - c;
        (1.0 - c - bb, bb, c)
    };
    let (ax, bx, cx) = coeffs(x1, x2);
    let (ay, by, cy) = coeffs(y1, y2);
    let sample_x = |t: f64| ((ax * t + bx) * t + cx) * t;
    let slope_x = |t: f64| (3.0 * ax * t + 2.0 * bx) * t + cx;
    let mut t = x;
    let mut solved = false;
    for _ in 0..8 {
        let err = sample_x(t) - x;
        if err.abs() < 1e-9 {
            solved = true;
            break;
        }
        let d = slope_x(t);
        if d.abs() < 1e-6 {
            break;
        }
        t -= err / d;
    }
    if !solved || !(0.0..=1.0).contains(&t) {
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        t = x;
        for _ in 0..64 {
            let v = sample_x(t);
            if (v - x).abs() < 1e-9 {
                break;
            }
            if v < x {
                lo = t;
            } else {
                hi = t;
            }
            t = (lo + hi) * 0.5;
        }
    }
    ((ay * t + by) * t + cy) * t
}

/// `steps(n, position)` at `x`, as the easing specification gives it.
fn steps(n: u32, pos: StepPos, x: f64) -> f64 {
    let n = f64::from(n.max(1));
    let mut step = (x * n).floor();
    if matches!(pos, StepPos::JumpStart | StepPos::JumpBoth) {
        step += 1.0;
    }
    if step < 0.0 {
        step = 0.0;
    }
    let jumps = match pos {
        StepPos::JumpStart | StepPos::JumpEnd => n,
        StepPos::JumpNone => (n - 1.0).max(1.0),
        StepPos::JumpBoth => n + 1.0,
    };
    if step > jumps {
        step = jumps;
    }
    step / jumps
}

// ----- running items ---------------------------------------------------------

#[derive(Clone, Copy)]
struct Running {
    element: ElementId,
    prop: PropId,
    from: AnimValue,
    to: AnimValue,
    start: u64,
    delay: f64,
    duration: f64,
    timing: Timing,
}

#[derive(Clone)]
struct Key {
    offset: f32,
    value: AnimValue,
    /// `animation-timing-function` written in this keyframe: it shapes the
    /// interval that starts here.
    timing: Option<Timing>,
}

#[derive(Clone)]
struct Track {
    prop: PropId,
    keys: Vec<Key>,
}

struct Playing {
    element: ElementId,
    spec: Animation,
    /// Tick at which its clock read zero.
    clock_start: u64,
    /// Elapsed ticks while paused.
    frozen: Option<u64>,
    tracks: Vec<Track>,
    /// Finished and holding its end state (or shown as such by reduced
    /// motion): nothing left to change.
    settled: bool,
    /// Reduced motion: always show the end state.
    end_only: bool,
    /// Scratch for matching against a new animation list.
    keep: bool,
}

impl Playing {
    fn live(&self) -> bool {
        !self.settled && self.frozen.is_none()
    }
}

struct Sample {
    /// Directed progress through the keyframes; `None` when the animation
    /// has no effect at this time.
    progress: Option<f64>,
    done: bool,
}

fn sample(spec: &Animation, elapsed: f64, end: bool) -> Sample {
    let duration = f64::from(spec.duration.max(0.0));
    let delay = f64::from(spec.delay);
    let iters = match spec.iterations {
        IterationCount::Count(c) => f64::from(c.max(0.0)),
        IterationCount::Infinite => f64::INFINITY,
    };
    let backwards = matches!(spec.fill, FillMode::Backwards | FillMode::Both);
    let forwards = end || matches!(spec.fill, FillMode::Forwards | FillMode::Both);
    let total = if duration <= 0.0 || iters <= 0.0 {
        0.0
    } else {
        duration * iters
    };
    let after = end || elapsed >= delay + total;
    let (iteration, within) = if after {
        if !forwards {
            return Sample {
                progress: None,
                done: true,
            };
        }
        if iters <= 0.0 {
            (0.0, 0.0)
        } else if !iters.is_finite() || duration <= 0.0 {
            (0.0, 1.0)
        } else {
            let frac = iters.fract();
            if frac == 0.0 {
                (iters - 1.0, 1.0)
            } else {
                (iters.floor(), frac)
            }
        }
    } else if elapsed < delay {
        if !backwards {
            return Sample {
                progress: None,
                done: false,
            };
        }
        (0.0, 0.0)
    } else {
        let overall = (elapsed - delay) / duration;
        let i = overall.floor();
        (i, overall - i)
    };
    let odd = (iteration % 2.0) >= 1.0;
    let reversed = match spec.direction {
        AnimDirection::Normal => false,
        AnimDirection::Reverse => true,
        AnimDirection::Alternate => odd,
        AnimDirection::AlternateReverse => !odd,
    };
    Sample {
        progress: Some(if reversed { 1.0 - within } else { within }),
        done: after,
    }
}

fn track_value(track: &Track, progress: f64, timing: Timing) -> Option<AnimValue> {
    let keys = track.keys.as_slice();
    let i = keys.iter().rposition(|k| f64::from(k.offset) <= progress)?;
    let a = keys.get(i)?;
    let Some(b) = keys.get(i + 1) else {
        return Some(a.value);
    };
    let span = f64::from(b.offset) - f64::from(a.offset);
    let local = if span > 0.0 {
        (progress - f64::from(a.offset)) / span
    } else {
        1.0
    };
    Some(AnimValue::lerp(
        a.value,
        b.value,
        ease(a.timing.unwrap_or(timing), local) as f32,
    ))
}

// ----- the animator ----------------------------------------------------------

/// All running transitions and animations of a screen.
pub(crate) struct Animator {
    now: u64,
    hz: f64,
    reduce: bool,
    transitions: Vec<Running>,
    anims: Vec<Playing>,
    /// Elements whose boxes may have moved since layout last looked.
    pub(crate) layout_dirty: Vec<ElementId>,
    paint_dirty: bool,
    /// Problems found (unknown animation names), for the screen to keep.
    pub(crate) warnings: Vec<MarkupError>,
    values: Vec<(PropId, AnimValue)>,
    /// The page's file name, for messages.
    pub(crate) file: String,
}

impl Default for Animator {
    fn default() -> Self {
        Animator {
            now: 0,
            hz: 60.0,
            reduce: false,
            transitions: Vec::new(),
            anims: Vec::new(),
            layout_dirty: Vec::new(),
            paint_dirty: false,
            warnings: Vec::new(),
            values: Vec::new(),
            file: String::new(),
        }
    }
}

const ANIM_PROPS: [PropId; 8] = [
    PropId::AnimationName,
    PropId::AnimationDuration,
    PropId::AnimationDelay,
    PropId::AnimationIterationCount,
    PropId::AnimationDirection,
    PropId::AnimationFillMode,
    PropId::AnimationTimingFunction,
    PropId::AnimationPlayState,
];

impl Animator {
    pub(crate) fn set_rate(&mut self, hz: f64) {
        if hz.is_finite() && hz > 0.0 {
            self.hz = hz;
        }
    }

    pub(crate) fn rate(&self) -> f64 {
        self.hz
    }

    pub(crate) fn reduce_motion(&self) -> bool {
        self.reduce
    }

    pub(crate) fn is_animating(&self) -> bool {
        !self.transitions.is_empty() || self.anims.iter().any(Playing::live)
    }

    /// Whether anything needs drawing again; clears the flag.
    pub(crate) fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.paint_dirty)
    }

    fn elapsed(&self, start: u64) -> f64 {
        self.now.saturating_sub(start) as f64 / self.hz
    }

    fn mark(&mut self, element: ElementId, prop: PropId) {
        self.paint_dirty = true;
        if effect_of(prop) == Effect::Layout && !self.layout_dirty.contains(&element) {
            self.layout_dirty.push(element);
        }
    }

    fn transition_value(&self, t: &Running) -> Option<AnimValue> {
        let elapsed = self.elapsed(t.start);
        let x = (elapsed - t.delay) / t.duration;
        if x >= 1.0 {
            None
        } else if x <= 0.0 {
            Some(t.from)
        } else {
            Some(AnimValue::lerp(t.from, t.to, ease(t.timing, x) as f32))
        }
    }

    /// Advances one tick and updates `used`. True if anything on screen
    /// changed. Allocates nothing.
    pub(crate) fn tick(
        &mut self,
        doc: &Document,
        styles: &Styles,
        used: &mut [ComputedStyle],
    ) -> bool {
        self.now += 1;
        if self.transitions.is_empty() && self.anims.is_empty() {
            return false;
        }
        self.transitions.retain(|t| doc.contains(t.element));
        self.anims.retain(|a| doc.contains(a.element));
        // Put everything back to the computed value, then lay the running
        // pieces on top in order.
        for t in &self.transitions {
            reset(used, styles, t.element, t.prop);
        }
        for a in &self.anims {
            for tr in &a.tracks {
                reset(used, styles, a.element, tr.prop);
            }
        }
        let mut i = 0;
        while let Some(t) = self.transitions.get(i).copied() {
            match self.transition_value(&t) {
                Some(v) => {
                    write(used, t.element, t.prop, v);
                    i += 1;
                }
                None => {
                    self.transitions.swap_remove(i);
                }
            }
            self.mark(t.element, t.prop);
        }
        let mut i = 0;
        while i < self.anims.len() {
            let keep = self.step_animation(i, used);
            if keep {
                i += 1;
            } else {
                self.anims.remove(i);
            }
        }
        std::mem::take(&mut self.paint_dirty)
    }

    /// Evaluates animation `i` at the current tick into `used`; false when
    /// it is finished and should go.
    fn step_animation(&mut self, i: usize, used: &mut [ComputedStyle]) -> bool {
        let Some(a) = self.anims.get(i) else {
            return false;
        };
        let live = a.live();
        let elapsed = match a.frozen {
            Some(f) => f as f64 / self.hz,
            None => self.elapsed(a.clock_start),
        };
        let s = sample(&a.spec, elapsed, a.end_only);
        let element = a.element;
        let timing = a.spec.timing;
        if let Some(p) = s.progress {
            for tr in &a.tracks {
                if let Some(v) = track_value(tr, p, timing) {
                    write(used, element, tr.prop, v);
                }
            }
        }
        let n = self.anims.get(i).map_or(0, |a| a.tracks.len());
        if live {
            for k in 0..n {
                let prop = self
                    .anims
                    .get(i)
                    .and_then(|a| a.tracks.get(k))
                    .map(|t| t.prop);
                if let Some(prop) = prop {
                    self.mark(element, prop);
                }
            }
        }
        if s.done {
            if s.progress.is_none() {
                return false;
            }
            if let Some(a) = self.anims.get_mut(i) {
                a.settled = true;
            }
        }
        true
    }

    /// Called for each element a restyle changed, after `used` was copied
    /// from the new computed style. Starts, replaces and cancels
    /// transitions and animations, then puts what is running for the element
    /// on top of `used`.
    pub(crate) fn on_change(
        &mut self,
        ch: &StyleChange,
        doc: &mut Document,
        styles: &mut Styles,
        used: &mut [ComputedStyle],
    ) {
        let el = ch.element;
        let Some(new) = styles.style(el).cloned() else {
            return;
        };
        if !ch.is_new {
            self.start_transitions(ch, &new);
        }
        let anim_changed = ch.is_new || ANIM_PROPS.iter().any(|p| !new.prop_eq(&ch.old, *p));
        if anim_changed {
            self.reconcile_animations(el, &new, doc, styles, used);
        }
        self.apply_element(el, used);
    }

    fn start_transitions(&mut self, ch: &StyleChange, new: &ComputedStyle) {
        let el = ch.element;
        for p in new.changed_props(&ch.old) {
            let (Some(old_v), Some(new_v)) = (ch.old.get(p), new.get(p)) else {
                continue;
            };
            let mut spec = None;
            for t in new.transitions() {
                match t.prop {
                    TransProp::All => spec = Some(t),
                    TransProp::Prop(q) if q == p => spec = Some(t),
                    TransProp::Prop(_) => {}
                }
            }
            let running = self
                .transitions
                .iter()
                .position(|t| t.element == el && t.prop == p);
            let from = running
                .and_then(|i| self.transitions.get(i))
                .and_then(|t| self.transition_value(t))
                .unwrap_or(old_v);
            // Turned round part way, back to where it began: it takes the
            // time it has already taken, not its full length again.
            let taken = running
                .and_then(|i| self.transitions.get(i))
                .filter(|t| t.from == new_v)
                .map(|t| self.elapsed(t.start) - t.delay)
                .filter(|taken| *taken > 0.0);
            if let Some(i) = running {
                self.transitions.swap_remove(i);
            }
            match spec {
                Some(s) if s.duration > 0.0 && !self.reduce => {
                    self.transitions.push(Running {
                        element: el,
                        prop: p,
                        from,
                        to: new_v,
                        start: self.now,
                        delay: if taken.is_some() {
                            0.0
                        } else {
                            f64::from(s.delay)
                        },
                        duration: taken.map_or(f64::from(s.duration), |taken| {
                            f64::from(s.duration).min(taken)
                        }),
                        timing: s.timing,
                    });
                    self.mark(el, p);
                }
                _ => {
                    // Cancelled or never started: the new value shows at
                    // once, and it is a change like any other.
                    if running.is_some() {
                        self.mark(el, p);
                    }
                }
            }
        }
    }

    fn reconcile_animations(
        &mut self,
        el: ElementId,
        new: &ComputedStyle,
        doc: &mut Document,
        styles: &mut Styles,
        used: &mut [ComputedStyle],
    ) {
        let list: Vec<Animation> = new.animations().collect();
        for a in self.anims.iter_mut().filter(|a| a.element == el) {
            a.keep = false;
        }
        for spec in list {
            let found = self
                .anims
                .iter()
                .position(|a| a.element == el && !a.keep && a.spec.name == spec.name);
            if let Some(i) = found {
                let now = self.now;
                if let Some(a) = self.anims.get_mut(i) {
                    a.keep = true;
                    a.spec = spec;
                    match (spec.play_state, a.frozen) {
                        (PlayState::Paused, None) => {
                            a.frozen = Some(now.saturating_sub(a.clock_start))
                        }
                        (PlayState::Running, Some(f)) => {
                            a.clock_start = now.saturating_sub(f);
                            a.frozen = None;
                        }
                        _ => {}
                    }
                }
            } else if let Some(p) = self.start_animation(el, spec, doc, styles) {
                self.anims.push(p);
            }
        }
        let mut i = 0;
        while i < self.anims.len() {
            let drop_it = self
                .anims
                .get(i)
                .is_some_and(|a| a.element == el && !a.keep);
            if drop_it {
                if let Some(a) = self.anims.get(i) {
                    for tr in &a.tracks {
                        reset(used, styles, el, tr.prop);
                        self.paint_dirty = true;
                        if effect_of(tr.prop) == Effect::Layout && !self.layout_dirty.contains(&el)
                        {
                            self.layout_dirty.push(el);
                        }
                    }
                }
                self.anims.remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn start_animation(
        &mut self,
        el: ElementId,
        spec: Animation,
        doc: &mut Document,
        styles: &mut Styles,
    ) -> Option<Playing> {
        let end_only = self.reduce;
        if end_only {
            let finite = matches!(spec.iterations, IterationCount::Count(_));
            let holds = matches!(spec.fill, FillMode::Forwards | FillMode::Both);
            if !(finite && holds) {
                return None;
            }
        }
        let Some((file, frames)) = styles.keyframes(spec.name) else {
            let (line, column) = doc.get(el).map_or((1, 1), |n| (n.line, n.column));
            let name = doc.interner().resolve(spec.name).to_owned();
            self.warnings.push(MarkupError::new(
                &self.file,
                line.max(1),
                column.max(1),
                format!("no @keyframes called `{name}`"),
                "write `@keyframes name { from { ... } to { ... } }` in a style sheet, or fix the name in animation-name",
            ));
            return None;
        };
        let file = file.to_owned();
        let stops: Vec<KeyframeStop> = frames.stops.clone();
        let base = styles.style(el).cloned()?;
        let mut tracks: Vec<Track> = Vec::new();
        for stop in &stops {
            self.values.clear();
            let decls: &[Declaration] = &stop.declarations;
            let stop_timing = decls.iter().rev().find_map(|d| match &d.value {
                DeclValue::Value(Value::Timings(l))
                    if d.prop == PropId::AnimationTimingFunction =>
                {
                    l.first().copied()
                }
                _ => None,
            });
            let mut out = std::mem::take(&mut self.values);
            styles.declared_values(doc, el, &file, decls, &mut out);
            for (prop, value) in out.drain(..) {
                let at = match tracks.iter().position(|t| t.prop == prop) {
                    Some(i) => i,
                    None => {
                        tracks.push(Track {
                            prop,
                            keys: Vec::new(),
                        });
                        tracks.len() - 1
                    }
                };
                if let Some(track) = tracks.get_mut(at) {
                    match track.keys.iter_mut().find(|k| k.offset == stop.offset) {
                        Some(k) => {
                            k.value = value;
                            k.timing = stop_timing;
                        }
                        None => track.keys.push(Key {
                            offset: stop.offset,
                            value,
                            timing: stop_timing,
                        }),
                    }
                }
            }
            self.values = out;
        }
        for track in &mut tracks {
            if let Some(b) = base.get(track.prop) {
                if !track.keys.iter().any(|k| k.offset <= 0.0) {
                    track.keys.insert(
                        0,
                        Key {
                            offset: 0.0,
                            value: b,
                            timing: None,
                        },
                    );
                }
                if !track.keys.iter().any(|k| k.offset >= 1.0) {
                    track.keys.push(Key {
                        offset: 1.0,
                        value: b,
                        timing: None,
                    });
                }
            }
            track.keys.sort_by(|a, b| a.offset.total_cmp(&b.offset));
        }
        let paused = spec.play_state == PlayState::Paused;
        Some(Playing {
            element: el,
            spec,
            clock_start: self.now,
            frozen: if paused { Some(0) } else { None },
            tracks,
            settled: end_only,
            end_only,
            keep: true,
        })
    }

    /// Writes what is running for `el` on top of its `used` style.
    fn apply_element(&mut self, el: ElementId, used: &mut [ComputedStyle]) {
        for k in 0..self.transitions.len() {
            let Some(t) = self.transitions.get(k).copied().filter(|t| t.element == el) else {
                continue;
            };
            if let Some(v) = self.transition_value(&t) {
                write(used, el, t.prop, v);
            }
        }
        for k in 0..self.anims.len() {
            let Some(a) = self.anims.get(k).filter(|a| a.element == el) else {
                continue;
            };
            let elapsed = match a.frozen {
                Some(f) => f as f64 / self.hz,
                None => self.elapsed(a.clock_start),
            };
            let s = sample(&a.spec, elapsed, a.end_only);
            if let Some(p) = s.progress {
                for tr in &a.tracks {
                    if let Some(v) = track_value(tr, p, a.spec.timing) {
                        write(used, el, tr.prop, v);
                    }
                }
                let props: Vec<PropId> = a.tracks.iter().map(|t| t.prop).collect();
                for prop in props {
                    self.mark(el, prop);
                }
            }
        }
    }

    pub(crate) fn set_reduce(&mut self, reduce: bool) {
        self.reduce = reduce;
    }

    /// Reduced motion was switched on: finish transitions now, and let
    /// animations either hold their end state or stop.
    pub(crate) fn snap_to_end(&mut self, styles: &Styles, used: &mut [ComputedStyle]) {
        let transitions = std::mem::take(&mut self.transitions);
        for t in &transitions {
            reset(used, styles, t.element, t.prop);
            self.mark(t.element, t.prop);
        }
        self.transitions = transitions;
        self.transitions.clear();
        let mut anims = std::mem::take(&mut self.anims);
        anims.retain_mut(|a| {
            let finite = matches!(a.spec.iterations, IterationCount::Count(_));
            let holds = matches!(a.spec.fill, FillMode::Forwards | FillMode::Both);
            for tr in &a.tracks {
                reset(used, styles, a.element, tr.prop);
                self.mark(a.element, tr.prop);
            }
            if finite && holds {
                a.end_only = true;
                a.settled = true;
                if let Some(s) = sample(&a.spec, 0.0, true).progress {
                    for tr in &a.tracks {
                        if let Some(v) = track_value(tr, s, a.spec.timing) {
                            write(used, a.element, tr.prop, v);
                        }
                    }
                }
                true
            } else {
                false
            }
        });
        self.anims = anims;
    }
}

fn reset(used: &mut [ComputedStyle], styles: &Styles, el: ElementId, prop: PropId) {
    if let (Some(slot), Some(base)) = (used.get_mut(el.index as usize), styles.style(el)) {
        slot.copy_prop(prop, base);
    }
}

fn write(used: &mut [ComputedStyle], el: ElementId, prop: PropId, v: AnimValue) {
    if let Some(slot) = used.get_mut(el.index as usize) {
        slot.set(prop, v);
    }
}
