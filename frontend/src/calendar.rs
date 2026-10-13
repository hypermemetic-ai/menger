//! FILTER-27: menger's own **calendar** and **time selector** — for
//! choosing a moment (the filter menu's window editor, src/when_view.rs;
//! the inputs page and future forms can take them too). Not the browser's
//! native date input, whose popup no theme reaches: both are drawn from the
//! theme's tokens (THEME-2), so they follow every theme, light and dark.
//!
//! - **[`Calendar`]**: a month grid, Monday first — ‹ › step the months, the
//!   year opens a menu of years, today is marked (`aria-current="date"`).
//!   One day chosen, or (given `end`) a **range**: the first click is its
//!   start, the second its end (before the start, the two swap), and the
//!   days between are tinted. The ARIA grid pattern: one cell in the tab
//!   order (roving), ← → a day, ↑ ↓ a week, PageUp / PageDown a month
//!   (with Shift a year), Home / End the week's ends, Enter or Space
//!   chooses.
//! - **[`TimeSelect`]**: hours and minutes, each a spin button — typed, or
//!   stepped with ↑ ↓ (Shift: by 10 minutes) — in the reader's 12- or
//!   24-hour clock ([`reader_h12`]), with am · pm in the 12-hour one.
//!
//! The date arithmetic, the grid, the keys and the range are pure functions
//! here, pinned by native tests.

use leptos::prelude::*;
use menger_tickets_api::filter::Moment;
use wasm_bindgen::JsCast;

/// A calendar day.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Day {
    pub(crate) y: i32,
    pub(crate) m: u32,
    pub(crate) d: u32,
}

pub(crate) const MONTH_NAMES: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

pub(crate) const WEEKDAYS: [(&str, &str); 7] =
    [("Mo", "Monday"), ("Tu", "Tuesday"), ("We", "Wednesday"), ("Th", "Thursday"), ("Fr", "Friday"), ("Sa", "Saturday"), ("Su", "Sunday")];

/// How many days month `m` of year `y` has.
pub(crate) fn days_in(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

/// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (yoe + era * 400 + i64::from(m <= 2)) as i32;
    (y, m, d)
}

impl Day {
    pub(crate) fn new(y: i32, m: u32, d: u32) -> Self {
        Day { y, m, d: d.clamp(1, days_in(y, m)) }
    }

    fn ordinal(self) -> i64 {
        days_from_civil(self.y, self.m, self.d)
    }

    fn of_ordinal(z: i64) -> Self {
        let (y, m, d) = civil_from_days(z);
        Day { y, m, d }
    }

    /// Monday 0 … Sunday 6.
    pub(crate) fn weekday(self) -> u32 {
        // 1970-01-01 was a Thursday (3).
        ((self.ordinal() + 3).rem_euclid(7)) as u32
    }

    pub(crate) fn plus_days(self, n: i64) -> Self {
        Day::of_ordinal(self.ordinal() + n)
    }

    /// `n` months on, the day kept where the month has it (else its last).
    pub(crate) fn plus_months(self, n: i32) -> Self {
        let total = self.y * 12 + (self.m as i32 - 1) + n;
        let (y, m) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
        Day::new(y, m, self.d)
    }

    /// `2026-10-01`.
    pub(crate) fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }

    /// `Thursday, October 1, 2026` — a cell's name.
    pub(crate) fn long(self) -> String {
        format!("{}, {} {}, {}", WEEKDAYS[self.weekday() as usize].1, MONTH_NAMES[self.m as usize - 1], self.d, self.y)
    }

    /// The language's moment: this day, at `time` when given.
    pub(crate) fn moment(self, time: Option<(u8, u8)>) -> Moment {
        Moment { year: self.y as i16, month: self.m as u8, day: self.d as u8, time }
    }

    pub(crate) fn of_moment(m: &Moment) -> Self {
        Day::new(i32::from(m.year), u32::from(m.month), u32::from(m.day))
    }
}

/// The month `(y, m)` as weeks, Monday first: each a day of the month or
/// `None` before its first and after its last.
pub(crate) fn grid(y: i32, m: u32) -> Vec<[Option<Day>; 7]> {
    let first = Day::new(y, m, 1);
    let lead = first.weekday() as usize;
    let n = days_in(y, m) as usize;
    let mut out = Vec::new();
    let mut week = [None; 7];
    for slot in 0..lead + n {
        if slot >= lead {
            week[slot % 7] = Some(Day::new(y, m, (slot - lead + 1) as u32));
        }
        if slot % 7 == 6 {
            out.push(week);
            week = [None; 7];
        }
    }
    if week.iter().any(Option::is_some) {
        out.push(week);
    }
    out
}

/// Where a binding moves the focused day in the grid — `None` for one that
/// moves nothing (the ARIA grid pattern's date-picker keys; AGS-28: the
/// table's `date.*`).
pub(crate) fn key_move(at: Day, id: &str) -> Option<Day> {
    Some(match id {
        "date.prev-day" => at.plus_days(-1),
        "date.next-day" => at.plus_days(1),
        "date.prev-week" => at.plus_days(-7),
        "date.next-week" => at.plus_days(7),
        "date.prev-year" => at.plus_months(-12),
        "date.next-year" => at.plus_months(12),
        "date.prev-month" => at.plus_months(-1),
        "date.next-month" => at.plus_months(1),
        "date.week-start" => at.plus_days(-i64::from(at.weekday())),
        "date.week-end" => at.plus_days(6 - i64::from(at.weekday())),
        _ => return None,
    })
}

/// A click on `day` in a range: no range or a whole one, a new start; a
/// start alone, the end — the two swapped when it is earlier.
pub(crate) fn range_click(start: Option<Day>, end: Option<Day>, day: Day) -> (Option<Day>, Option<Day>) {
    match (start, end) {
        (Some(s), None) if day < s => (Some(day), Some(s)),
        (Some(s), None) => (Some(s), Some(day)),
        _ => (Some(day), None),
    }
}

/// Whether `day` is strictly between a range's ends (tinted).
pub(crate) fn between(day: Day, start: Option<Day>, end: Option<Day>) -> bool {
    matches!((start, end), (Some(s), Some(e)) if s < day && day < e)
}

/// A time stepped: `part` 0 the hours (wrapping a day), 1 the minutes
/// (wrapping an hour, the hour kept).
pub(crate) fn step_time((h, m): (u8, u8), part: u8, by: i32) -> (u8, u8) {
    if part == 0 {
        ((i32::from(h) + by).rem_euclid(24) as u8, m)
    } else {
        (h, (i32::from(m) + by).rem_euclid(60) as u8)
    }
}

/// An hour as typed: 0–23, or 1–12 with `pm` in the 12-hour clock.
pub(crate) fn read_hour(text: &str, h12: bool, pm: bool) -> Option<u8> {
    let n: u8 = text.trim().parse().ok()?;
    if !h12 {
        return (n < 24).then_some(n);
    }
    match n {
        1..=11 => Some(if pm { n + 12 } else { n }),
        12 => Some(if pm { 12 } else { 0 }),
        _ => None,
    }
}

/// An hour as the 12-hour clock shows it.
pub(crate) fn hour12(h: u8) -> u8 {
    match h % 12 {
        0 => 12,
        x => x,
    }
}

/// Today, in the reader's zone.
pub(crate) fn today() -> Day {
    let (d, _) = now_local();
    d
}

/// Now, in the reader's zone: the day, and the time to the minute.
pub(crate) fn now_local() -> (Day, (u8, u8)) {
    let now = js_sys::Date::new_0();
    let d = Day::new(now.get_full_year() as i32, now.get_month() + 1, now.get_date());
    (d, (now.get_hours() as u8, now.get_minutes() as u8))
}

/// Whether the reader's clock is the 12-hour one (its locale's hour cycle).
pub(crate) fn reader_h12() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        thread_local! {
            static H12: bool = {
                let opts = js_sys::Object::new();
                let _ = js_sys::Reflect::set(&opts, &"hour".into(), &"numeric".into());
                let options = js_sys::Intl::DateTimeFormat::new(&js_sys::Array::new(), &opts).resolved_options();
                let cycle = js_sys::Reflect::get(&options, &"hourCycle".into()).ok().and_then(|z| z.as_string());
                let h12 = js_sys::Reflect::get(&options, &"hour12".into()).ok().and_then(|z| z.as_bool());
                match cycle.as_deref() {
                    Some("h11" | "h12") => true,
                    Some(_) => false,
                    None => h12.unwrap_or(false),
                }
            };
        }
        H12.with(|h| *h)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

// ---------------------------------------------------------------------
// The views

/// Focus the cell of `day` in the calendar `id`, once drawn.
fn focus_day(id: String, day: Day) {
    request_animation_frame(move || {
        let Some(doc) = crate::dom::window().document() else { return };
        let sel = format!("#{id} [data-day=\"{}\"]", day.iso());
        if let Some(el) = doc.query_selector(&sel).ok().flatten().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = el.focus();
        }
    });
}

/// Focus the year button `id`, once the year menu is gone.
fn focus_year(id: String) {
    request_animation_frame(move || {
        let Some(doc) = crate::dom::window().document() else { return };
        if let Some(el) = doc.get_element_by_id(&id).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
            let _ = el.focus();
        }
    });
}

thread_local! {
    static NEXT_ID: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

fn fresh_id(prefix: &str) -> String {
    NEXT_ID.with(|n| {
        n.set(n.get() + 1);
        format!("{prefix}-{}", n.get())
    })
}

/// A month grid choosing `start` — or, given `end`, a range from `start`
/// to `end`. `label` names the grid for a screen reader.
#[component]
pub(crate) fn Calendar(
    start: RwSignal<Option<Day>>,
    #[prop(optional)] end: Option<RwSignal<Option<Day>>>,
    #[prop(into)] label: String,
) -> impl IntoView {
    let today = today();
    let id = fresh_id("cal");
    let first = start.get_untracked().unwrap_or(today);
    let view_ym = RwSignal::new((first.y, first.m));
    let focus = RwSignal::new(first);
    let years_open = RwSignal::new(false);
    let choose = move |d: Day| {
        match end {
            Some(end) => {
                let (s, e) = range_click(start.get_untracked(), end.get_untracked(), d);
                start.set(s);
                end.set(e);
            }
            None => start.set(Some(d)),
        }
        focus.set(d);
    };
    let go = {
        let id = id.clone();
        move |d: Day| {
            focus.set(d);
            view_ym.set((d.y, d.m));
            focus_day(id.clone(), d);
        }
    };
    let step_month = move |n: i32| {
        let (y, m) = view_ym.get_untracked();
        let d = Day::new(y, m, focus.get_untracked().d).plus_months(n);
        view_ym.set((d.y, d.m));
        focus.set(d);
    };
    let keydown = {
        let go = go.clone();
        move |ev: web_sys::KeyboardEvent| {
            let id = crate::bindings::hit(&ev, crate::bindings::Context::DatePicker).unwrap_or_default();
            if let Some(d) = key_move(focus.get_untracked(), id) {
                ev.prevent_default();
                ev.stop_propagation();
                go(d);
            } else if id == "date.choose" {
                ev.prevent_default();
                ev.stop_propagation();
                choose(focus.get_untracked());
            }
        }
    };
    let title_id = format!("{id}-t");
    let year_btn = format!("{id}-y");
    let year_sv = StoredValue::new(year_btn.clone());
    let years = move || {
        let (y, _) = view_ym.get();
        ((today.y - 10)..=(today.y + 2)).chain(std::iter::once(y)).collect::<std::collections::BTreeSet<_>>()
    };
    view! {
        <div class="cal" id=id.clone() data-range=end.is_some().to_string()>
            <div class="cal-head">
                <button type="button" class="cal-step" aria-label="previous month" on:click=move |_| step_month(-1)>"‹"</button>
                <span class="cal-title" id=title_id.clone() aria-live="polite">
                    {move || MONTH_NAMES[view_ym.get().1 as usize - 1]}
                </span>
                <button
                    type="button"
                    class="cal-year"
                    id=year_btn
                    aria-haspopup="listbox"
                    aria-expanded=move || years_open.get().to_string()
                    aria-label=move || format!("year {}, choose another", view_ym.get().0)
                    on:click=move |_| years_open.update(|o| *o = !*o)
                >
                    {move || view_ym.get().0}" ▾"
                </button>
                <button type="button" class="cal-step" aria-label="next month" on:click=move |_| step_month(1)>"›"</button>
            </div>
            {move || years_open.get().then(|| view! {
                <div
                    class="cal-years"
                    role="listbox"
                    aria-label="years"
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if crate::bindings::is(&ev, crate::bindings::Context::DatePicker, "dialog.close") {
                            ev.prevent_default();
                            ev.stop_propagation();
                            years_open.set(false);
                            focus_year(year_sv.get_value());
                        }
                    }
                >
                    {years().into_iter().map(|y| {
                        view! {
                        <button
                            type="button"
                            role="option"
                            class="cal-y"
                            aria-selected=move || (view_ym.get().0 == y).to_string()
                            on:click=move |_| {
                                let (_, m) = view_ym.get_untracked();
                                view_ym.set((y, m));
                                focus.set(Day::new(y, m, focus.get_untracked().d));
                                years_open.set(false);
                                focus_year(year_sv.get_value());
                            }
                        >
                            {y}
                        </button>
                    }}).collect_view()}
                </div>
            })}
            <table class="cal-grid" role="grid" aria-labelledby=title_id aria-label=label on:keydown=keydown>
                <thead>
                    <tr role="row">
                        {WEEKDAYS.iter().map(|(short, long)| view! {
                            <th role="columnheader" scope="col" abbr=*long>{*short}</th>
                        }).collect_view()}
                    </tr>
                </thead>
                <tbody>
                    {move || {
                        let (y, m) = view_ym.get();
                        grid(y, m).into_iter().map(|week| view! {
                            <tr role="row">
                                {week.into_iter().map(|cell| match cell {
                                    None => view! { <td role="gridcell" class="cal-pad" aria-hidden="true"></td> }.into_any(),
                                    Some(d) => {
                                        let chosen = move || {
                                            let (s, e) = (start.get(), end.and_then(|e| e.get()));
                                            Some(d) == s || Some(d) == e
                                        };
                                        let tinted = move || between(d, start.get(), end.and_then(|e| e.get()));
                                        view! {
                                            <td
                                                role="gridcell"
                                                class="cal-day"
                                                class:on=chosen
                                                class:span=tinted
                                                class:today=d == today
                                                data-day=d.iso()
                                                tabindex=move || if focus.get() == d { "0" } else { "-1" }
                                                aria-selected=move || chosen().to_string()
                                                aria-current=(d == today).then_some("date")
                                                aria-label=d.long()
                                                on:click=move |_| choose(d)
                                                on:focus=move |_| focus.set(d)
                                            >
                                                {d.d}
                                            </td>
                                        }
                                        .into_any()
                                    }
                                }).collect_view()}
                            </tr>
                        }).collect_view()
                    }}
                </tbody>
            </table>
        </div>
    }
}

/// Hours and minutes, typed or stepped, in the reader's clock (`h12`).
#[component]
pub(crate) fn TimeSelect(value: RwSignal<(u8, u8)>, h12: bool, #[prop(into)] label: String) -> impl IntoView {
    let hours_text = move || {
        let (h, _) = value.get();
        if h12 { hour12(h).to_string() } else { format!("{h:02}") }
    };
    let minutes_text = move || format!("{:02}", value.get().1);
    let spin = move |part: u8| {
        move |ev: web_sys::KeyboardEvent| {
            let by = match crate::bindings::hit(&ev, crate::bindings::Context::TimeField) {
                Some("time.up") => 1,
                Some("time.down") => -1,
                Some("time.up-ten") => 10,
                Some("time.down-ten") => -10,
                _ => return,
            };
            ev.prevent_default();
            ev.stop_propagation();
            value.update(|t| *t = step_time(*t, part, by));
        }
    };
    let typed_hours = move |ev: web_sys::Event| {
        let pm = value.get_untracked().0 >= 12;
        if let Some(h) = read_hour(&event_target_value(&ev), h12, pm) {
            value.update(|t| t.0 = h);
        }
    };
    let typed_minutes = move |ev: web_sys::Event| {
        if let Some(m) = event_target_value(&ev).trim().parse::<u8>().ok().filter(|m| *m < 60) {
            value.update(|t| t.1 = m);
        }
    };
    view! {
        <span class="tsel" role="group" aria-label=label>
            <input
                class="tsel-h"
                type="text"
                inputmode="numeric"
                role="spinbutton"
                aria-label="hours"
                aria-valuemin=if h12 { "1" } else { "0" }
                aria-valuemax=if h12 { "12" } else { "23" }
                aria-valuenow=move || if h12 { hour12(value.get().0) } else { value.get().0 }
                maxlength="2"
                prop:value=hours_text
                on:change=typed_hours
                on:keydown=spin(0)
            />
            <span class="tsel-sep" aria-hidden="true">":"</span>
            <input
                class="tsel-m"
                type="text"
                inputmode="numeric"
                role="spinbutton"
                aria-label="minutes"
                aria-valuemin="0"
                aria-valuemax="59"
                aria-valuenow=move || value.get().1
                maxlength="2"
                prop:value=minutes_text
                on:change=typed_minutes
                on:keydown=spin(1)
            />
            {h12.then(|| view! {
                <button
                    type="button"
                    class="tsel-ap"
                    aria-label=move || { if value.get().0 >= 12 { "pm — switch to am" } else { "am — switch to pm" } }
                    on:click=move |_| value.update(|t| t.0 = (t.0 + 12) % 24)
                >
                    {move || { if value.get().0 >= 12 { "pm" } else { "am" } }}
                </button>
            })}
        </span>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The calendar's arithmetic: weekdays, month lengths, a day on and a
    /// month on — against dates known.
    #[test]
    fn days_and_months() {
        assert_eq!(Day::new(2026, 10, 13).weekday(), 1, "2026-10-13 is a Tuesday");
        assert_eq!(Day::new(1970, 1, 1).weekday(), 3);
        assert_eq!(Day::new(2024, 2, 29).weekday(), 3);
        assert_eq!(days_in(2024, 2), 29);
        assert_eq!(days_in(2100, 2), 28);
        assert_eq!(days_in(2000, 2), 29);
        assert_eq!(Day::new(2026, 12, 31).plus_days(1), Day::new(2027, 1, 1));
        assert_eq!(Day::new(2026, 3, 1).plus_days(-1), Day::new(2026, 2, 28));
        assert_eq!(Day::new(2026, 1, 31).plus_months(1), Day::new(2026, 2, 28), "the month's last when it has no 31st");
        assert_eq!(Day::new(2026, 1, 15).plus_months(-13), Day::new(2024, 12, 15));
        assert_eq!(Day::new(2026, 10, 1).iso(), "2026-10-01");
        assert_eq!(Day::new(2026, 10, 1).long(), "Thursday, October 1, 2026");
        let m = Day::new(2026, 10, 1).moment(Some((8, 30)));
        assert_eq!(m.format(), "2026-10-01T08:30");
        assert_eq!(Day::of_moment(&m), Day::new(2026, 10, 1));
        for z in [-800_000i64, -1, 0, 1, 20_000, 400_000] {
            assert_eq!(Day::of_ordinal(z).ordinal(), z);
        }
    }

    /// October 2026 starts on a Thursday: three blank cells, five weeks.
    #[test]
    fn the_grid_is_monday_first() {
        let g = grid(2026, 10);
        assert_eq!(g.len(), 5);
        assert_eq!(g[0][..3], [None, None, None]);
        assert_eq!(g[0][3], Some(Day::new(2026, 10, 1)));
        assert_eq!(g[4][5], Some(Day::new(2026, 10, 31)));
        assert_eq!(g[4][6], None);
        assert_eq!(grid(2021, 2).len(), 4, "February 2021: Monday the 1st, four whole weeks");
        assert_eq!(grid(2026, 10).iter().flatten().flatten().count(), 31);
    }

    /// The ARIA grid pattern's keys.
    #[test]
    fn the_keys_move_the_focus() {
        let at = Day::new(2026, 10, 13);
        let key_move = |at, key: &str, shift: bool| {
            use crate::bindings::{pick_single, table, Context, Press};
            let p = Press { key: key.into(), shift, ..Press::default() };
            key_move(at, pick_single(&table(), &p, Context::DatePicker, false).unwrap_or_default())
        };
        assert_eq!(key_move(at, "ArrowLeft", false), Some(Day::new(2026, 10, 12)));
        assert_eq!(key_move(at, "ArrowRight", false), Some(Day::new(2026, 10, 14)));
        assert_eq!(key_move(at, "ArrowUp", false), Some(Day::new(2026, 10, 6)));
        assert_eq!(key_move(at, "ArrowDown", false), Some(Day::new(2026, 10, 20)));
        assert_eq!(key_move(at, "PageUp", false), Some(Day::new(2026, 9, 13)));
        assert_eq!(key_move(at, "PageDown", false), Some(Day::new(2026, 11, 13)));
        assert_eq!(key_move(at, "PageUp", true), Some(Day::new(2025, 10, 13)));
        assert_eq!(key_move(at, "Home", false), Some(Day::new(2026, 10, 12)), "Monday");
        assert_eq!(key_move(at, "End", false), Some(Day::new(2026, 10, 18)), "Sunday");
        assert_eq!(key_move(at, "Enter", false), None);
    }

    /// A range: the first click its start, the second its end (swapped
    /// when earlier), a third a new start; the days between tinted.
    #[test]
    fn a_range_is_two_clicks() {
        let (a, b, c) = (Day::new(2026, 10, 5), Day::new(2026, 10, 9), Day::new(2026, 10, 2));
        assert_eq!(range_click(None, None, a), (Some(a), None));
        assert_eq!(range_click(Some(a), None, b), (Some(a), Some(b)));
        assert_eq!(range_click(Some(a), None, c), (Some(c), Some(a)), "earlier: swapped");
        assert_eq!(range_click(Some(a), Some(b), c), (Some(c), None), "a whole range: a new start");
        assert!(between(Day::new(2026, 10, 7), Some(a), Some(b)));
        assert!(!between(a, Some(a), Some(b)) && !between(b, Some(a), Some(b)), "the ends are chosen, not tinted");
        assert!(!between(Day::new(2026, 10, 7), Some(a), None));
    }

    /// The time selector: stepped with wrapping, typed in either clock.
    #[test]
    fn the_time_steps_and_reads() {
        assert_eq!(step_time((23, 59), 0, 1), (0, 59));
        assert_eq!(step_time((0, 0), 1, -1), (0, 59), "minutes wrap within the hour");
        assert_eq!(step_time((8, 55), 1, 10), (8, 5));
        assert_eq!(read_hour("7", false, false), Some(7));
        assert_eq!(read_hour("24", false, false), None);
        assert_eq!(read_hour("12", true, false), Some(0), "12 am is midnight");
        assert_eq!(read_hour("12", true, true), Some(12));
        assert_eq!(read_hour("3", true, true), Some(15));
        assert_eq!(read_hour("13", true, true), None);
        assert_eq!((hour12(0), hour12(12), hour12(15)), (12, 12, 3));
    }
}
