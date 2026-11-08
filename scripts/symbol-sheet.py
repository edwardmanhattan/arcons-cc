#!/usr/bin/env python3
"""The contact sheet behind `docs/research/nato-symbology.md` (#33).

A JUDGEMENT AID, not a verifier. It draws the icons the sister project already curated, at the sizes
this wall could draw them, so the selection can be re-judged at *our* size rather than inherited on
the strength of a number. Once #32 lands, the real verification is a golden capture of the wall — a
sheet drawn by a second renderer cannot prove anything about the first.

Provenance matters here. The geometry is not ours and not TFG's: it comes from
`spatialillusions/milsymbol` (MIT, © 2017 Mans Beckman) via TFG's generated table, which is why
`THIRD-PARTY-NOTICES.md` carries the row. This script reads TFG's *generated* file directly rather
than a copy, so the sheet cannot drift from what TFG would draw.

Usage:

    scripts/symbol-sheet.py [--tfg DIR] [--out FILE]

Defaults: TFG at `~/Work/Crossnet/tfg`, output `docs/research/sheets/wall-symbol-icons.svg`.
"""

import argparse
import math
import pathlib
import re
import sys

SIZES = [12, 16, 24, 32, 40]  # points; 12 is MARKER, 24 is MARKER_TARGET, 32 is TFG's box
SURFACE = ["surface_combatant", "patrol_craft", "mine_warfare", "amphibious_warfare_ship",
           "carrier", "oiler_tanker", "hospital_ship", "auxiliary", "convoy", "unmanned_surface"]


def parse_generated(path):
    """{variant: [(kind, [(x, y), ...]), ...]} out of the generated Rust table.

    Split on the array literals rather than on the marks: an icon is one `&[ … ]` block, and an icon
    made only of strokes (the infantry saltire, the engineer bracket) has no `Fill` at all.
    """
    text = path.read_text()
    variants = re.findall(r"^\s{4}(\w+)\s*=\s*(\d+),", text, re.M)
    order = [name for name, _ in sorted(variants, key=lambda v: int(v[1]))]
    body = text.split("pub const GEOMETRY", 1)[1]
    blocks = re.findall(r"^    &\[(.*?)^    \],", body, re.S | re.M)
    icons = []
    for block in blocks:
        marks = []
        for kind, coords in re.findall(r"IconMark::(Fill|Stroke)\(&\[(.*?)\]\)", block, re.S):
            pts = [tuple(map(float, m)) for m in re.findall(r"\((-?[\d.]+),\s*(-?[\d.]+)\)", coords)]
            marks.append((kind, pts))
        icons.append(marks)
    return dict(zip(order, icons))


def parse_labels(path):
    """{variant: human label} and the manifest order, out of the .tsv."""
    labels, order = {}, []
    for line in path.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        cells = line.split("\t")
        if len(cells) < 3:
            continue
        variant, label = cells[0], cells[1]
        order.append(variant)
        labels[variant] = label
    return labels, order


def ink_extent(marks):
    xs = [p[0] for _, pts in marks for p in pts]
    ys = [p[1] for _, pts in marks for p in pts]
    return max(xs) - min(xs), max(ys) - min(ys)


def unused_segment_gap(marks):
    """The smallest distance between two segments of different marks, in icon units.

    One stroke wide is the smallest feature the eye can separate, so this is the number that decides
    whether two details are two details at a given size.
    """
    def segs(pts):
        return list(zip(pts, pts[1:] + pts[:1]))

    def point_seg(p, a, b):
        ax, ay = a
        bx, by = b
        dx, dy = bx - ax, by - ay
        length2 = dx * dx + dy * dy
        t = 0.0 if length2 == 0 else max(0.0, min(1.0, ((p[0] - ax) * dx + (p[1] - ay) * dy) / length2))
        q = (ax + t * dx, ay + t * dy)
        return math.dist(p, q)

    best = None
    groups = [segs(pts) for _, pts in marks]
    for i in range(len(groups)):
        for j in range(i + 1, len(groups)):
            for a, b in groups[i]:
                for c, d in groups[j]:
                    for p in (a, b):
                        for q in (c, d):
                            g = min(point_seg(p, c, d), point_seg(q, a, b))
                            best = g if best is None else min(best, g)
    return best


def svg_row(variant, label, marks, size):
    """One icon at one size, in a cell of `size + padding`, icon box centred."""
    pad = 6
    side = size + pad * 2
    stroke = max(0.75, 0.072 * size)
    w, h = ink_extent(marks)
    # The fit scales the longer axis to the box — TFG's rule, restated here so a sheet cell means the
    # same thing their map means.
    scale = size / max(w, h) if max(w, h) > 0 else 0.0
    out = [f'<g transform="translate({pad},{pad})">']
    for kind, pts in marks:
        # Authored in a unit square; centre it, scale by the fit, leave room for the stroke.
        local = size - stroke
        xy = [((x - 0.5) * local / 1.0 * (1.0 if max(w, h) == 0 else 1.0) + local / 2,
               (y - 0.5) * local / 1.0 + local / 2) for x, y in pts]
        # Re-scale about the icon's own ink centre so a long thin icon is not off-centre.
        mx = sum(p[0] for p in xy) / len(xy)
        my = sum(p[1] for p in xy) / len(xy)
        xs = [p[0] for p in xy]
        ys = [p[1] for p in xy]
        cx, cy = (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2
        path = "M" + " L".join(f"{x - cx + local / 2:.2f},{y - cy + local / 2:.2f}" for x, y in xy) + " Z"
        if kind == "Fill":
            out.append(f'<path d="{path}" fill="#cfe9ff" fill-rule="nonzero"/>')
        else:
            out.append(f'<path d="{path}" fill="none" stroke="#cfe9ff" stroke-width="{stroke:.2f}" '
                       f'stroke-linejoin="round"/>')
    out.append("</g>")
    return "".join(out), side


def main():
    here = pathlib.Path(__file__).resolve().parent.parent
    ap = argparse.ArgumentParser()
    ap.add_argument("--tfg", default=str(pathlib.Path.home() / "Work/Crossnet/tfg"))
    ap.add_argument("--out", default=str(here / "docs/research/sheets/wall-symbol-icons.svg"))
    args = ap.parse_args()

    tfg = pathlib.Path(args.tfg)
    icons = parse_generated(tfg / "src/symbology/icons_generated.rs")
    labels, order = parse_labels(tfg / "assets/symbology/icons.tsv")

    # The manifest is snake_case, the generated enum is CamelCase: one conversion, and the sheet cannot
    # accidentally draw a different icon from the one the manifest names.
    camel = {snake: "".join(part.capitalize() for part in snake.split("_")) for snake in order}
    order = [camel[snake] for snake in order]
    labels = {camel[snake]: label for snake, label in labels.items()}

    rows = [v for v in order if v in icons]
    unknown = [v for v in order if v not in icons]
    if unknown:
        print(f"# {unknown} have no upstream geometry (Unspecified draws an empty interior)", file=sys.stderr)

    left = 300
    top = 116
    width = max(1180, left + sum(s + 26 for s in SIZES) + 24)
    height = top + len(rows) * (SIZES[-1] + 20) + 40
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}" font-family="Inter, sans-serif">',
        f'<rect width="{width}" height="{height}" fill="#0b1620"/>',
        '<text x="20" y="34" fill="#e8f3ff" font-size="20">The wall\'s symbol vocabulary at the sizes '
        'it could be drawn (#33)</text>',
        '<text x="20" y="58" fill="#8fb0c8" font-size="13">Icon geometry from spatialillusions/milsymbol '
        '(MIT) via TFG\'s generated table. 12 pt is this wall\'s marker; 24 pt its touch target; 32 pt '
        'TFG\'s box.</text>',
        '<text x="20" y="80" fill="#8fb0c8" font-size="13">Judgement aid only: a second renderer '
        'cannot verify the first.</text>',
    ]
    x = left
    for size in SIZES:
        parts.append(f'<text x="{x}" y="{top - 8}" fill="#e8f3ff" font-size="13">{size} pt</text>')
        x += size + 26

    y = top
    for variant in rows:
        marks = icons[variant]
        colour = "#e8f3ff" if variant in SURFACE else "#8fb0c8"
        name = labels.get(variant, variant)
        parts.append(f'<text x="20" y="{y + SIZES[-1] / 2 + 4}" fill="{colour}" font-size="14">{name}</text>')
        x = left
        for size in SIZES:
            cell, side = svg_row(variant, name, marks, size)
            parts.append(f'<g transform="translate({x},{y + (SIZES[-1] - side) / 2})">{cell}</g>')
            x += size + 26
        y += SIZES[-1] + 20
    parts.append("</svg>")

    out = pathlib.Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(parts) + "\n")

    # The numbers the document quotes, printed so they can be checked rather than trusted. Every icon
    # is fitted to the box on its longer axis (TFG's rule), so its ink is `size` long by construction;
    # what differs between icons is how many separate strokes carry that ink.
    print(f"{'icon':26} {'marks':>5} {'strokes':>7} {'fit axis':>8}")
    for variant in rows:
        marks = icons[variant]
        w, h = ink_extent(marks)
        strokes = sum(1 for kind, _ in marks if kind == "Stroke")
        print(f"{variant:26} {len(marks):5} {strokes:7} {max(w, h):8.2f}")
    print(f"\n{len(rows)} icons with geometry; sheet at {out}")


if __name__ == "__main__":
    main()
