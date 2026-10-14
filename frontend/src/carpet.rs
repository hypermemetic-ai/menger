//! DESIGN-38: the Menger carpet — menger's mark, as the landing page draws
//! it (DESIGN-37's `.mark`): eight 9×9 squares in a 27×27 box, the centre
//! empty, each filled with a status of the lifecycle in order — scope,
//! pending, ready, coding, in review, complete, ready again — and the accent
//! last. The colours are the token contract's (`.carpet .cN` in
//! index.html), so a theme recolours it. The header's wordmark wears it;
//! the nav's pip is the same carpet, small and breathing (DESIGN-39 gives
//! its cells the live counts).

use leptos::prelude::*;

/// The eight cells: `(class, x, y)` in the 27×27 box, row by row, the
/// centre (9, 9) left out.
pub(crate) const CELLS: [(&str, u8, u8); 8] = [
    ("c1", 0, 0),
    ("c2", 9, 0),
    ("c3", 18, 0),
    ("c4", 0, 9),
    ("c6", 18, 9),
    ("c7", 0, 18),
    ("c8", 9, 18),
    ("c9", 18, 18),
];

/// The carpet as an inline SVG, decoration only (`aria-hidden`): `class`
/// places it (`mark` in the header, `pip` in the nav).
#[component]
pub(crate) fn Carpet(class: &'static str) -> impl IntoView {
    view! {
        <svg class=format!("carpet {class}") viewBox="0 0 27 27" aria-hidden="true" focusable="false">
            {CELLS
                .iter()
                .map(|&(c, x, y)| view! { <rect class=c x=x.to_string() y=y.to_string() width="9" height="9" /> })
                .collect_view()}
        </svg>
    }
}

/// DESIGN-39: the lifecycle's five as the pip's classes (`s-<slug>`),
/// in `motion::LIFECYCLE`'s order.
pub(crate) const SLUGS: [&str; 5] = ["pending", "ready", "coding", "in-review", "complete"];

/// DESIGN-39: the nav's pip — the carpet whose eight cells take the live
/// mix of the lifecycle among what the nav shows (`cells`: each cell's
/// index into [`SLUGS`]); the plain carpet until the counts are in.
#[component]
pub(crate) fn Pip(#[prop(into)] cells: Signal<Option<[usize; 8]>>) -> impl IntoView {
    let mix = move || {
        cells.get().map(|m| {
            let mut out: Vec<String> = Vec::new();
            for (k, slug) in SLUGS.iter().enumerate() {
                let n = m.iter().filter(|&&i| i == k).count();
                if n > 0 {
                    out.push(format!("{slug}:{n}"));
                }
            }
            out.join(" ")
        })
    };
    view! {
        <svg class="carpet pip" data-mix=mix viewBox="0 0 27 27" aria-hidden="true" focusable="false">
            {CELLS
                .iter()
                .enumerate()
                .map(|(i, &(c, x, y))| {
                    let class = move || match cells.get() {
                        Some(m) => format!("{c} s-{}", SLUGS[m[i].min(SLUGS.len() - 1)]),
                        None => c.to_string(),
                    };
                    view! { <rect class=class x=x.to_string() y=y.to_string() width="9" height="9" /> }
                })
                .collect_view()}
        </svg>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Eight cells tiling the box but its centre, and every cell painted by
    /// a token in the stylesheet.
    #[test]
    fn eight_cells_the_centre_empty_each_a_token() {
        let mut seen = std::collections::HashSet::new();
        for (_, x, y) in CELLS {
            assert!(x % 9 == 0 && y % 9 == 0 && x < 27 && y < 27);
            assert!((x, y) != (9, 9), "the centre stays empty");
            assert!(seen.insert((x, y)));
        }
        let css = crate::tokens_lint::app_css();
        for (c, _, _) in CELLS {
            let rule = format!(".carpet .{c} {{ fill: var(--");
            assert!(css.contains(&rule), "index.html paints {c} with a token: {rule}");
        }
    }
}
