"""Charts for the new examples/complete benchmarks (07-10).

Generates .bench-charts/09-complete-benchmarks.png (matplotlib, dark style
matching 01-08) and appends page 9 to .bench-charts/spectralang-benchmarks.pdf
(reportlab vector bars + pypdf merge; pages 1-8 untouched).

Data: wall time in ms, median of 5 runs on the author's Windows x64 box.
Spectra includes JIT compile; Go/Rust are prebuilt optimized binaries.
Workloads are small, so process startup dominates absolute times; the
checksums below prove semantic parity across the triple.

Usage: python scripts/bench_complete_charts.py
"""
import io
import math
import os

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHARTS = os.path.join(REPO, ".bench-charts")
PDF_PATH = os.path.join(CHARTS, "spectralang-benchmarks.pdf")
PNG_PATH = os.path.join(CHARTS, "09-complete-benchmarks.png")

# name -> (spectra_ms, go_ms, rust_ms, workload, checksum)
DATA = {
    "sieve-500": (166.8, 11.9, 10.3,
                  "sieve to 500, 200 rounds", "primes=95 total=19000"),
    "hashmap-500": (232.4, 18.0, 13.9,
                    "500 inserts + 500 lookups, 200 rounds", "total=100000"),
    "json-20": (55.0, 12.3, 9.4,
                "20 JSON roundtrips, 5 rounds", "objs=100 sum_ids=950"),
    "matmul-32": (50.2, 13.0, 11.8,
                  "naive 32x32, 20 rounds, order i,j,k", "sum=55869440"),
}

BG = "#101418"
SPECTRA = "#4DA3FF"
GO = "#FFBC42"
RUST = "#3FE488"
TEXT = "#E8EDF2"
GREY = "#8D98A6"

CAPTION = ("wall time, median of 5 runs (ms, log scale) - Spectra incl. JIT compile - "
           "Go/Rust prebuilt -O binaries - checksums prove parity")


def make_png(path=PNG_PATH):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    names = list(DATA.keys())
    s_vals = [DATA[n][0] for n in names]
    g_vals = [DATA[n][1] for n in names]
    r_vals = [DATA[n][2] for n in names]

    fig, ax = plt.subplots(figsize=(15.11, 9.8), dpi=100)
    fig.patch.set_facecolor(BG)
    ax.set_facecolor(BG)
    y = range(len(names))
    h = 0.22
    ax.barh([i + h for i in y], s_vals, height=h, color=SPECTRA, label="Spectra")
    ax.barh(list(y), g_vals, height=h, color=GO, label="Go")
    ax.barh([i - h for i in y], r_vals, height=h, color=RUST, label="Rust")
    ax.set_xscale("log")
    ax.set_yticks(list(y))
    ax.set_yticklabels(names, color=TEXT, fontsize=13)
    ax.tick_params(axis="x", colors=GREY, labelsize=11)
    ax.set_xlabel("ms (log scale, median of 5 runs)", color=GREY, fontsize=12)
    ax.set_title("examples/complete - new benchmarks (Spectra vs Go vs Rust)",
                 color=TEXT, fontsize=17, pad=14)
    for i, n in enumerate(names):
        s, g, r, workload, checksum = DATA[n]
        ax.text(s * 1.08, i + h, "%.1fx vs Go" % (s / g),
                va="center", color=TEXT, fontsize=11)
    leg = ax.legend(frameon=False, loc="lower right", fontsize=12)
    for t in leg.get_texts():
        t.set_color(TEXT)
    fig.text(0.5, 0.02, CAPTION, ha="center", color=GREY, fontsize=10)
    for spine in ax.spines.values():
        spine.set_color(GREY)
    ax.grid(axis="x", color="#2A3138", linestyle="--", alpha=0.7)
    fig.tight_layout(rect=(0, 0.04, 1, 0.96))
    fig.savefig(path)
    print("wrote", path)


def _draw_pdf_page(canvas_path=None):
    from reportlab.pdfgen import canvas as rl_canvas
    from reportlab.lib.colors import HexColor

    buf = io.BytesIO()
    W, H = 864, 486
    c = rl_canvas.Canvas(buf, pagesize=(W, H))
    c.setFillColor(HexColor(BG))
    c.rect(0, 0, W, H, fill=1, stroke=0)
    c.setFillColor(HexColor(TEXT))
    c.setFont("Helvetica-Bold", 20)
    c.drawString(36, H - 44, "examples/complete - new benchmarks (Spectra vs Go vs Rust)")
    c.setFillColor(HexColor(GREY))
    c.setFont("Helvetica", 11)
    c.drawString(36, H - 64, CAPTION)

    names = list(DATA.keys())
    top, bottom = H - 100, 56
    row_h = (top - bottom) / len(names)
    bar_max_w = W - 300
    log_min, log_max = math.log10(8), math.log10(320)

    def x_of(ms):
        t = (math.log10(ms) - log_min) / (log_max - log_min)
        return 220 + t * bar_max_w

    # x ticks at 10 / 100 ms
    c.setFont("Helvetica", 10)
    for tick in (10, 100):
        x = x_of(tick)
        c.setStrokeColor(HexColor("#2A3138"))
        c.line(x, bottom, x, top)
        c.setFillColor(HexColor(GREY))
        c.drawCentredString(x, bottom - 16, "%d" % tick)
    c.drawCentredString(220 + bar_max_w / 2, bottom - 30, "ms (log scale)")

    # legend
    lx = W - 220
    for j, (label, color) in enumerate(
            (("Spectra", SPECTRA), ("Go", GO), ("Rust", RUST))):
        c.setFillColor(HexColor(color))
        c.rect(lx, top - 8 - j * 20, 14, 10, fill=1, stroke=0)
        c.setFillColor(HexColor(TEXT))
        c.setFont("Helvetica", 11)
        c.drawString(lx + 20, top - 8 - j * 20, label)

    for i, n in enumerate(names):
        s, g, r, workload, checksum = DATA[n]
        cy = top - i * row_h - row_h / 2
        c.setFillColor(HexColor(TEXT))
        c.setFont("Helvetica-Bold", 12)
        c.drawString(36, cy + 22, n)
        c.setFillColor(HexColor(GREY))
        c.setFont("Helvetica", 9)
        c.drawString(36, cy + 8, workload)
        c.drawString(36, cy - 5, "checksum " + checksum)
        bh = 13
        for j, (val, color) in enumerate(
                ((s, SPECTRA), (g, GO), (r, RUST))):
            y = cy - 22 + (2 - j) * (bh + 3)
            c.setFillColor(HexColor(color))
            c.rect(220, y, max(x_of(val) - 220, 2), bh, fill=1, stroke=0)
        c.setFillColor(HexColor(TEXT))
        c.setFont("Helvetica", 10)
        c.drawString(x_of(s) + 6, cy - 22 + 2 * (bh + 3),
                     "%.1f/%.1f/%.1f ms  %.1fx vs Go" % (s, g, r, s / g))
    c.showPage()
    c.save()
    buf.seek(0)
    return buf


def append_pdf_page(pdf_path=PDF_PATH):
    from pypdf import PdfReader, PdfWriter

    reader = PdfReader(pdf_path)
    assert len(reader.pages) == 8, "expected 8 pages, got %d" % len(reader.pages)
    writer = PdfWriter()
    for page in reader.pages:
        writer.add_page(page)
    writer.add_page(PdfReader(_draw_pdf_page()).pages[0])
    with open(pdf_path, "wb") as f:
        writer.write(f)
    print("rewrote", pdf_path, "pages:", len(writer.pages))


def append_exec_page(pdf_path=PDF_PATH):
    from pypdf import PdfReader, PdfWriter

    reader = PdfReader(pdf_path)
    assert len(reader.pages) == 9, "expected 9 pages, got %d" % len(reader.pages)
    writer = PdfWriter()
    for page in reader.pages:
        writer.add_page(page)
    writer.add_page(PdfReader(_draw_pdf_page_exec()).pages[0])
    with open(pdf_path, "wb") as f:
        writer.write(f)
    print("rewrote", pdf_path, "pages:", len(writer.pages))


def drop_generated_pages(pdf_path=PDF_PATH, keep=8):
    # Rebuild support: discards our own generated pages (9+) so fresh data
    # can be appended below. Pages 1-8 are never touched.
    from pypdf import PdfReader, PdfWriter

    reader = PdfReader(pdf_path)
    assert len(reader.pages) > keep, "expected > %d pages, got %d" % (
        keep, len(reader.pages))
    writer = PdfWriter()
    for page in reader.pages[:keep]:
        writer.add_page(page)
    with open(pdf_path, "wb") as f:
        writer.write(f)
    print("truncated", pdf_path, "pages:", len(writer.pages))


# Exec-only gaps vs Go: differential (wall_big - wall_small) / delta_work
# cancels the fixed JIT-compile + process-startup cost. json is non-linear,
# so it uses per-object time on the 600-object workload instead.
# name -> (gap_vs_go, spectra_unit, go_unit, unit, cause)
DATA_EXEC = {
    "sieve-500": (1.7, "1.11us", "0.67us", "por iteracao",
                  "so loop JIT; 14.0x no wall era custo fixo"),
    "hashmap-500": (15.2, "0.95ms", "0.06ms", "por round",
                    "map via host call (custo real da linguagem)"),
    "json-20": (5.7, "0.11ms", "0.02ms", "por objeto, carga 600",
                "1 call de encode + 5 de decode por roundtrip"),
    "matmul-32": (0.6, "12.6us", "20.5us", "por round",
                  "Spectra mais rapido que Go no loop puro"),
}

PNG_EXEC_PATH = os.path.join(CHARTS, "10-complete-benchmarks-exec-only.png")

CAPTION_EXEC = ("gap de execucao vs Go (custo fixo cancelado por diferencial) - "
                "linha 1.0 = paridade - json por objeto na carga de 600")


def make_png_exec(path=PNG_EXEC_PATH):
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    names = list(DATA_EXEC.keys())
    gaps = [DATA_EXEC[n][0] for n in names]
    colors = [RUST if g < 1.0 else ("#FF5D5D" if g > 100 else
                                    (GO if g > 5 else SPECTRA)) for g in gaps]

    fig, ax = plt.subplots(figsize=(15.11, 9.8), dpi=100)
    fig.patch.set_facecolor(BG)
    ax.set_facecolor(BG)
    y = range(len(names))
    ax.barh(list(y), gaps, height=0.45, color=colors)
    ax.set_yticks(list(y))
    ax.set_yticklabels(names, color=TEXT, fontsize=14)
    ax.tick_params(axis="x", colors=GREY, labelsize=11)
    ax.set_xlabel("gap de execucao Spectra / Go (custo fixo cancelado)",
                  color=GREY, fontsize=12)
    ax.set_title("Cargas grandes: o que sobra sem o custo fixo (Spectra vs Go)",
                 color=TEXT, fontsize=17, pad=14)
    ax.axvline(1.0, color=GREY, linestyle="--", linewidth=1.2)
    xmax = max(gaps) * 1.12
    ax.set_xlim(0, xmax)
    for i, n in enumerate(names):
        gap, s_unit, g_unit, unit, cause = DATA_EXEC[n]
        ax.text(gap * 1.02, i,
                "%.1fx  (%s vs %s %s) - %s" % (gap, s_unit, g_unit, unit, cause),
                va="center", color=TEXT, fontsize=11)
    fig.text(0.5, 0.02, CAPTION_EXEC, ha="center", color=GREY, fontsize=10)
    for spine in ax.spines.values():
        spine.set_color(GREY)
    ax.grid(axis="x", color="#2A3138", linestyle="--", alpha=0.7)
    fig.tight_layout(rect=(0, 0.04, 1, 0.96))
    fig.savefig(path)
    print("wrote", path)


def _draw_pdf_page_exec():
    from reportlab.pdfgen import canvas as rl_canvas
    from reportlab.lib.colors import HexColor

    buf = io.BytesIO()
    W, H = 864, 486
    c = rl_canvas.Canvas(buf, pagesize=(W, H))
    c.setFillColor(HexColor(BG))
    c.rect(0, 0, W, H, fill=1, stroke=0)
    c.setFillColor(HexColor(TEXT))
    c.setFont("Helvetica-Bold", 20)
    c.drawString(36, H - 44, "Cargas grandes: o que sobra sem o custo fixo (Spectra vs Go)")
    c.setFillColor(HexColor(GREY))
    c.setFont("Helvetica", 11)
    c.drawString(36, H - 64, CAPTION_EXEC)

    names = list(DATA_EXEC.keys())
    top, bottom = H - 100, 56
    row_h = (top - bottom) / len(names)
    bar_max_w = W - 360
    gmax = max(DATA_EXEC[n][0] for n in names) * 1.08

    def x_of(gap):
        return 220 + gap / gmax * bar_max_w

    c.setFont("Helvetica", 10)
    for tick in (1, 100, 200, 300):
        x = x_of(tick)
        c.setStrokeColor(HexColor("#2A3138"))
        c.line(x, bottom, x, top)
        c.setFillColor(HexColor(GREY))
        c.drawCentredString(x, bottom - 16, "%dx" % tick)
    c.setStrokeColor(HexColor(GREY))
    c.setDash(4, 3)
    c.line(x_of(1.0), bottom, x_of(1.0), top)
    c.setDash()
    c.drawCentredString(220 + bar_max_w / 2, bottom - 30, "gap Spectra / Go (1.0 = paridade)")

    for i, n in enumerate(names):
        gap, s_unit, g_unit, unit, cause = DATA_EXEC[n]
        cy = top - i * row_h - row_h / 2
        c.setFillColor(HexColor(TEXT))
        c.setFont("Helvetica-Bold", 12)
        c.drawString(36, cy + 18, n)
        c.setFillColor(HexColor(GREY))
        c.setFont("Helvetica", 9)
        c.drawString(36, cy + 4, cause)
        if gap < 1.0:
            color = RUST
        elif gap > 100:
            color = "#FF5D5D"
        elif gap > 5:
            color = GO
        else:
            color = SPECTRA
        c.setFillColor(HexColor(color))
        c.rect(220, cy - 26, max(x_of(gap) - 220, 2), 16, fill=1, stroke=0)
        c.setFillColor(HexColor(TEXT))
        c.setFont("Helvetica", 10)
        c.drawString(x_of(gap) + 6, cy - 22,
                     "%.1fx (%s vs %s %s)" % (gap, s_unit, g_unit, unit))
    c.showPage()
    c.save()
    buf.seek(0)
    return buf

if __name__ == "__main__":
    from pypdf import PdfReader

    make_png()
    make_png_exec()
    npages = len(PdfReader(PDF_PATH).pages)
    if npages > 8:
        drop_generated_pages()
        npages = 8
    if npages == 8:
        append_pdf_page()
        append_exec_page()
    elif npages == 9:
        append_exec_page()
    else:
        print("PDF already has %d pages, nothing to append" % npages)

