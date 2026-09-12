#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Gerador de PDF técnico: "Arquitetura da Linguagem SpectraLang"
Python + reportlab 4.5.x  (documento A4, estilo slides empacotados).

Uso:
    python presentation/spectra_architecture_pdf.py
Gera:
    presentation/spectra_architecture.pdf
"""

import os
from reportlab.lib.pagesizes import A4
from reportlab.lib.units import cm, mm
from reportlab.lib import colors
from reportlab.lib.styles import getSampleStyleSheet, ParagraphStyle
from reportlab.lib.enums import TA_CENTER, TA_LEFT
from reportlab.platypus import (
    SimpleDocTemplate, Paragraph, Spacer, Table, TableStyle,
    Preformatted, PageBreak, KeepTogether, HRFlowable,
)
from reportlab.graphics.shapes import Drawing, Rect, String, Line, Polygon

# --------------------------------------------------------------------------- #
# Paleta de cores (marca Spectra)
# --------------------------------------------------------------------------- #
PRIMARY   = colors.HexColor("#4F46E5")  # indigo
SECONDARY = colors.HexColor("#7C3AED")  # violet
ACCENT    = colors.HexColor("#0EA5E9")  # sky
DARK      = colors.HexColor("#1E293B")
GREY      = colors.HexColor("#64748B")
LIGHT_BG  = colors.HexColor("#F1F5F9")
CODE_BG   = colors.HexColor("#0F172A")
CODE_FG   = colors.HexColor("#E2E8F0")
GREEN     = colors.HexColor("#10B981")
WHITE     = colors.white

# --------------------------------------------------------------------------- #
# Estilos de texto
# --------------------------------------------------------------------------- #
ss = getSampleStyleSheet()

style_title = ParagraphStyle(
    "TitleX", parent=ss["Title"], fontName="Helvetica-Bold",
    fontSize=23, leading=27, textColor=PRIMARY, spaceAfter=6)
style_subtitle = ParagraphStyle(
    "SubX", parent=ss["Normal"], fontName="Helvetica",
    fontSize=12, leading=16, textColor=DARK, spaceAfter=4)
style_h1 = ParagraphStyle(
    "H1X", parent=ss["Heading1"], fontName="Helvetica-Bold",
    fontSize=15, leading=19, textColor=PRIMARY, spaceBefore=2, spaceAfter=6)
style_h2 = ParagraphStyle(
    "H2X", parent=ss["Heading2"], fontName="Helvetica-Bold",
    fontSize=11.5, leading=15, textColor=SECONDARY, spaceBefore=8, spaceAfter=3)
style_body = ParagraphStyle(
    "BodyX", parent=ss["Normal"], fontName="Helvetica",
    fontSize=9.6, leading=13.5, textColor=DARK, spaceAfter=5, alignment=TA_LEFT)
style_bullet = ParagraphStyle(
    "BulletX", parent=style_body, leftIndent=12, bulletIndent=2,
    spaceAfter=2.5)
style_caption = ParagraphStyle(
    "CapX", parent=ss["Normal"], fontName="Helvetica-Oblique",
    fontSize=7.8, leading=10, textColor=GREY, spaceBefore=2, spaceAfter=8)
style_code = ParagraphStyle(
    "CodeX", fontName="Courier", fontSize=7.6, leading=9.8,
    textColor=CODE_FG, backColor=CODE_BG, borderPadding=6,
    leftIndent=4, spaceBefore=3, spaceAfter=8)
style_foot = ParagraphStyle(
    "FootX", parent=ss["Normal"], fontName="Helvetica",
    fontSize=7.5, leading=9, textColor=GREY)
style_card = ParagraphStyle(
    "CardX", parent=ss["Normal"], fontName="Helvetica",
    fontSize=9, leading=12.5, textColor=DARK)


def esc(t: str) -> str:
    """Escapa entidades XML para uso em Preformatted/Paragraph."""
    return (t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;"))


# --------------------------------------------------------------------------- #
# Diagramas (reportlab.graphics)
# --------------------------------------------------------------------------- #
def _arrow_head(x, y, direction):
    if direction == "right":
        return Polygon([x, y - 3, x - 6, y, x, y + 3],
                       fillColor=GREY, strokeColor=None)
    if direction == "down":
        return Polygon([x - 3, y, x, y + 6, x + 3, y],
                       fillColor=GREY, strokeColor=None)
    if direction == "left":
        return Polygon([x, y - 3, x + 6, y, x, y + 3],
                       fillColor=GREY, strokeColor=None)
    # up
    return Polygon([x - 3, y, x, y - 6, x + 3, y],
                   fillColor=GREY, strokeColor=None)


def flow_diagram(items, cols=5, box_w=86, box_h=46, h_gap=18, v_gap=34,
                 title_color=PRIMARY):
    """Layout de caixas com setas (direita na linha, baixo entre linhas)."""
    rows = (len(items) + cols - 1) // cols
    width = cols * box_w + (cols - 1) * h_gap
    height = rows * box_h + (rows - 1) * v_gap
    d = Drawing(width, height)
    pos = {}
    for idx, it in enumerate(items):
        r, c = divmod(idx, cols)
        x = c * (box_w + h_gap)
        y = height - (r + 1) * box_h - r * v_gap
        pos[idx] = (x, y, box_w, box_h)
        d.add(Rect(x, y, box_w, box_h, rx=6, ry=6,
                   fillColor=it.get("color", title_color), strokeColor=None))
        d.add(String(x + box_w / 2, y + box_h / 2 + 6, it["label"],
                     fontSize=8.5, fillColor=WHITE, textAnchor="middle",
                     fontName="Helvetica-Bold"))
        if it.get("sub"):
            d.add(String(x + box_w / 2, y + box_h / 2 - 7, it["sub"],
                         fontSize=6.2, fillColor=WHITE, textAnchor="middle",
                         fontName="Helvetica"))
    for idx in range(len(items) - 1):
        r, c = divmod(idx, cols)
        if c < cols - 1:
            x1 = pos[idx][0] + box_w
            x2 = pos[idx + 1][0]
            y = pos[idx][1] + box_h / 2
            d.add(Line(x1, y, x2 - 7, y, strokeColor=GREY, strokeWidth=1.2))
            d.add(_arrow_head(x2 - 7, y, "right"))
        else:
            x = pos[idx][0] + box_w / 2
            y1 = pos[idx][1]
            y2 = pos[idx + 1][1] + box_h
            d.add(Line(x, y1, x, y2 + 6, strokeColor=GREY, strokeWidth=1.2))
            d.add(_arrow_head(x, y2 + 6, "down"))
    return d


# --------------------------------------------------------------------------- #
# Cabeçalho / rodapé
# --------------------------------------------------------------------------- #
SECTION_TITLES = []


def _on_page(canvas, doc):
    canvas.saveState()
    # barra superior
    canvas.setFillColor(PRIMARY)
    canvas.rect(0, A4[1] - 14 * mm, A4[0], 14 * mm, fill=1, stroke=0)
    canvas.setFillColor(WHITE)
    canvas.setFont("Helvetica-Bold", 9)
    canvas.drawString(18 * mm, A4[1] - 9 * mm, "SpectraLang · Arquitetura & Processo de Compilação")
    canvas.setFont("Helvetica", 7.5)
    canvas.drawRightString(A4[0] - 18 * mm, A4[1] - 9 * mm, "v0.2.6")
    # rodapé
    canvas.setStrokeColor(LIGHT_BG)
    canvas.setLineWidth(0.6)
    canvas.line(18 * mm, 14 * mm, A4[0] - 18 * mm, 14 * mm)
    canvas.setFillColor(GREY)
    canvas.setFont("Helvetica", 7.5)
    canvas.drawString(18 * mm, 10 * mm, "Documento técnico-comercial · gerado via Python/reportlab")
    canvas.drawRightString(A4[0] - 18 * mm, 10 * mm, "Página %d" % doc.page)
    canvas.restoreState()


# --------------------------------------------------------------------------- #
# Blocos de conteúdo reutilizáveis
# --------------------------------------------------------------------------- #
def slide(num, title):
    SECTION_TITLES.append(title)
    bar = HRFlowable(width="100%", thickness=2, color=SECONDARY,
                     spaceBefore=2, spaceAfter=4)
    t = Paragraph("Slide %02d &nbsp;·&nbsp; %s" % (num, esc(title)), style_h1)
    return [bar, t]


def code(text, caption=None):
    flow = [Preformatted(esc(text), style_code)]
    if caption:
        flow.append(Paragraph(esc(caption), style_caption))
    return flow


def bullets(items):
    return [Paragraph("• " + esc(i), style_bullet) for i in items]


def card_table(rows, col_widths, header=True):
    t = Table(rows, colWidths=col_widths)
    style = [
        ("FONTNAME", (0, 0), (-1, -1), "Helvetica"),
        ("FONTSIZE", (0, 0), (-1, -1), 8.3),
        ("TEXTCOLOR", (0, 0), (-1, -1), DARK),
        ("VALIGN", (0, 0), (-1, -1), "MIDDLE"),
        ("ROWBACKGROUNDS", (0, 1 if header else 0), (-1, -1),
         [WHITE, LIGHT_BG]),
        ("LINEBELOW", (0, 0), (-1, -1), 0.4, colors.HexColor("#CBD5E1")),
        ("TOPPADDING", (0, 0), (-1, -1), 4),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 4),
        ("LEFTPADDING", (0, 0), (-1, -1), 6),
    ]
    if header:
        style += [
            ("BACKGROUND", (0, 0), (-1, 0), PRIMARY),
            ("TEXTCOLOR", (0, 0), (-1, 0), WHITE),
            ("FONTNAME", (0, 0), (-1, 0), "Helvetica-Bold"),
        ]
    t.setStyle(TableStyle(style))
    return t


# --------------------------------------------------------------------------- #
# Conteúdo dos slides
# --------------------------------------------------------------------------- #
def build():
    f = []

    # 1 - CAPA ---------------------------------------------------------------
    f += [Spacer(1, 30 * mm)]
    f += [Paragraph("SpectraLang", style_title)]
    f += [Paragraph("Arquitetura da Linguagem e Processo de Compilação", style_subtitle)]
    f += [Spacer(1, 4 * mm)]
    f += [HRFlowable(width="62%", thickness=2, color=SECONDARY, spaceAfter=8)]
    f += [Paragraph(
        "Uma linguagem e toolchain implementada em Rust para cargas de "
        "<b>IA/ML</b> e para a construção nativa de <b>APIs e serviços</b> "
        "HTTP/event-driven — um único compilador, um único runtime, um único binário.",
        style_body)]
    f += [Spacer(1, 6 * mm)]
    f += [Paragraph("Apresentação técnica-comercial · CLI v0.2.6 · MIT", style_caption)]
    f += [PageBreak()]

    # 2 - O PROBLEMA --------------------------------------------------------
    f += slide(2, "O Problema: a costura que quebra")
    f += [Paragraph(
        "Times de IA escrevem o modelo em <b>Python</b> (PyTorch/JAX) e reescrevem "
        "a camada de serviço em <b>Go / Rust / Node</b>. Resultado: duas linguagens, "
        "dois toolchains, dois conjuntos de habilidades e uma fronteira de deployment "
        "propensa a divergência de tipos, de performance e de comportamento.", style_body)]
    _t = card_table([
        ["Etapa", "Linguagem típica", "Custo / risco"],
        ["Treinamento / experimento", "Python", "Ecossistema maduro, mas lento em produção"],
        ["Inferência / serving", "Go, Rust, Node", "Reimplementação + drift de lógica"],
        ["Tooling (lint, fmt, pkg, LSP)", "Fragmentado", "N× integrações e atrito de DX"],
    ], [55 * mm, 45 * mm, 67 * mm])
    f.append(_t)
    f += [Spacer(1, 4 * mm)]
    f += [Paragraph(
        "<b>Tese da SpectraLang:</b> a mesma linguagem que descreve o tensor deve "
        "descrever a requisição HTTP — treino e serving no mesmo arquivo, mesmo "
        "runtime, mesmo artefato de deployment.", style_body)]
    f += [PageBreak()]

    # 3 - A TESE / VISÃO ----------------------------------------------------
    f += slide(3, "A Tese: uma linguagem, um runtime, um binário")
    f += [Paragraph(
        "Dois workstreams deliberadamente acoplados compartilham compilador, runtime "
        "e toolchain:", style_body)]
    f.append(card_table([
        ["Workstream", "Escopo"],
        ["AI/ML core", "Tensor-first na linguagem; tipos científicos; ops ciente de forma; "
                       "autodiff reverso; framework ML (módulos, losses, otimizadores, datasets); "
                       "aceleradores; interop NumPy/ONNX"],
        ["API platform (spectra.api)", "async/await de 1ª classe; primitivos HTTP tipados; "
                       "routing, middleware, JSON, TLS, drivers de DB; observabilidade — "
                       "entregue como pacote versionado, não como std"],
    ], [42 * mm, 125 * mm]))
    f += [Spacer(1, 4 * mm)]
    f += [Paragraph(
        "Decisão arquitetural-chave: <font name='Courier'>spectra.api</font> é um "
        "<b>pacote versionado separado</b>, não parte do <font name='Courier'>std</font>, "
        "para que padrões web evoluam no próprio ritmo sem inflar cada build com "
        "TLS/drivers/exportadores.", style_body)]
    f += [PageBreak()]

    # 4 - PANORAMA DA SINTAXE ----------------------------------------------
    f += slide(4, "Panorama da Linguagem: familiar à primeira vista")
    f += [Paragraph(
        "Semântica tipo Rust (traits, <font name='Courier'>match</font>, "
        "<font name='Courier'>dyn</font>, <font name='Courier'>&amp;self</font>) + "
        "ergonomia tipo Python (<font name='Courier'>import as</font>, "
        "<font name='Courier'>elif</font>, f-strings) + API estilo PyTorch. "
        "Originais: <b>unless</b>, <b>module</b> obrigatório, visibilidade "
        "<font name='Courier'>pub/internal/privada</font>.", style_body)]
    f += code(
        """module demo;
import std.io;
import { println } from std.io;

let x: int = 10;          // explícito
let y = 20;               // inferido
let msg = f"Soma = {x + y}";

fn double(x: int) -> int { x * 2 }   // retorno implícito

fn classify(v: int) -> int {
    unless v < 0 {
        switch v {
            case 0 => { return 10; }
            else   => { return 30; }
        }
    } else { return -10; }
}

enum State { Ready(int), Serving, Failed(int) }
match s {
    State::Ready(n) => n,
    State::Serving   => 100,
    _                => 0,
}
""", "Fonte: SYNTAX_SUMMARY.md, tests/validation/120_stable_promoted_control_flow.spectra")
    f += [PageBreak()]

    # 5 - DIFERENCIAL 1: TENSORES -------------------------------------------
    f += slide(5, "Diferencial 1: tensores no sistema de tipos")
    f += [Paragraph(
        "<b>dtype, rank, dimensões, layout de memória e dispositivo</b> são parte do "
        "tipo e verificados em tempo de compilação (códigos "
        "<font name='Courier'>E1401–E1406</font>). PyTorch pega o erro de forma no "
        "époco 3; SpectraLang pega antes de rodar.", style_body)]
    f += code(
        """fn total(values: Tensor<float, rank1, dim4, row_major, cpu>) -> int {
    return tensor.sum_f(values) as int;
}

let w: Tensor<float, rank2, dim2, dim1, row_major, cpu> = [[2.0], [2.0]];
let loss: Tensor<float, rank0> = diff {        // região diferenciável nativa
    tensor.sum_t(tensor.mul(v, v))
};
let grad = tensor.grad(v);                      // autodiff de 1ª classe
""", "Fonte: tests/validation/102_pattern_tensor_ai_composition_stress.spectra")
    f += [Paragraph(
        "<font name='Courier'>diff { }</font> é um <b>construct de linguagem</b> que "
        "rebaixa para <font name='Courier'>std.tensor.backward</font> — autodiff é "
        "recurso do compilador, não biblioteca de gravação de tape.", style_body)]
    f += [PageBreak()]

    # 6 - DIFERENCIAL 2: TREINAR E SERVIR -----------------------------------
    f += slide(6, "Diferencial 2: treinar e servir em um arquivo")
    f += [Paragraph(
        "O mesmo arquivo .spectra treina o modelo e sobe o servidor de inferência — "
        "67 linhas, sem costura de linguagem.", style_body)]
    f += code(
        """import std.tensor as tensor;
import std.ml as ml;
import std.serve as serve;

fn train_step(x, target, w, b) -> int {
    let pred = ml.linear(x, w, b);
    let loss = ml.mse_loss(pred, target);
    tensor.backward(loss);
    ml.sgd_step(w, 0.1); ml.sgd_step(b, 0.1);
    return 0;
}

pub fn main() -> int {
    tensor.set_grad_enabled(true);
    let model = ml.module_new();
    let dataset = ml.dataset_from_tensors(x, target, 4);
    let loader  = ml.dataloader_new(dataset, 2, 2026);  // seed => reprodutível
    // ... loop de treino ...
    let server = serve.server_new(2);
    serve.server_warmup(server);
    serve.server_enqueue(server, 21);
    serve.server_process_batch(server, 1);
    return 0;
}
""", "Fonte: examples/ai/mlp_training_serving.spectra")
    f += [PageBreak()]

    # 7 - DIFERENCIAL 3: ASYNC ----------------------------------------------
    f += slide(7, "Diferencial 3: async/await + reactor de plataforma")
    f += [Paragraph(
        "Async é conceito de <b>primeira classe na IR</b> "
        "(<font name='Courier'>AsyncReady</font>, "
        "<font name='Courier'>Type::Task</font>); a espera usa o reactor "
        "(<font name='Courier'>mio</font> → <b>epoll</b> (Linux) / "
        "<b>IOCP</b> (Windows) / <b>kqueue</b> (macOS)) via host call "
        "<font name='Courier'>spectra.async.task.wait</font>.", style_body)]
    f += code(
        """async fn add_one() -> int {
    let value = await ready_value();   // await prefixado
    return value + 1;
}

async fn from_block() -> int {
    let task: Task<int> = async {       // bloco async => Task<T>
        let b = await ready_value();
        b + 1
    };
    return await task;
}
""", "Fonte: tests/validation/121_async_await_lowering.spectra")
    f += [Paragraph(
        "20 códigos de diagnóstico <font name='Courier'>E2101–E2120</font> garantem "
        "segurança de async no compilador (valores não-Send através de "
        "<font name='Courier'>await</font>, traits async não object-safe, etc.).", style_body)]
    f += [PageBreak()]

    # 8 - ARQUITETURA GERAL ------------------------------------------------
    f += slide(8, "Arquitetura Geral: quatro crates, um seam")
    f += [Paragraph(
        "Workspace Cargo (9 crates). O trait <font name='Courier'>BackendDriver</font> "
        "é a dobradura arquitetural: <font name='Courier'>spectra-compiler</font> tem "
        "<b>zero</b> dependência de Cranelift/runtime (só serde_json), logo é "
        "embutível — é assim que o LSP o reusa.", style_body)]
    diag = flow_diagram([
        {"label": "spectra-compiler", "sub": "front-end", "color": PRIMARY},
        {"label": "spectra-midend", "sub": "IR / opt", "color": SECONDARY},
        {"label": "spectra-backend", "sub": "Cranelift", "color": ACCENT},
        {"label": "spectra-runtime", "sub": "serviços", "color": GREEN},
    ], cols=4, box_w=104, box_h=50, h_gap=16)
    f += [diag, Spacer(1, 3 * mm)]
    f += [Paragraph(
        "tools/: <font name='Courier'>spectra-cli</font> (binário "
        "<font name='Courier'>spectralang</font>), <font name='Courier'>spectra-lsp</font>, "
        "<font name='Courier'>spectra-interop</font>. Pacotes: "
        "<font name='Courier'>spectra-api</font>, <font name='Courier'>spectra-db</font>.",
        style_caption)]
    f += [PageBreak()]

    # 9 - FRONT-END ---------------------------------------------------------
    f += slide(9, "Front-end: lexer -> parser -> AST -> semântica -> lint")
    f += bullets([
        "Lexer: autômato escrito à mão (tokens com spans byte+linha); f-strings, comentários de bloco, operadores compostos.",
        "Parser: descida recursiva, 1 token de lookahead, com recuperação de erro (synchronize) que reporta múltiplos erros por passada.",
        "ModuleLoader: cache incremental — hash(source+features) evita re-lexar/re-parser em hit.",
        "Análise Semântica: visitor multipassada (imports -> declarações -> corpos -> genéricos -> tipos de MethodCall); symbol stack + ModuleRegistry compartilhado.",
        "Lint: 3 regras (unused-binding, unreachable-code, shadowing); regra 'deny' vira erro de compilação.",
    ])
    f += code(
        """.spectra
   │
 [1] LEXING      compiler/src/lexer        -> Vec<Token>
 [2] PARSING     compiler/src/parser       -> Module (AST)
 [3] SEMANTIC    compiler/src/semantic     -> Vec<SemanticError>
 [4] LINT        compiler/src/lint         -> Vec<LintDiagnostic>
   │
 BackendDriver::run(&ast, &options)
""")
    f += [PageBreak()]

    # 10 - MID-END ----------------------------------------------------------
    f += slide(10, "Mid-end: IR SSA (SIR) + autodiff + otimização")
    f += [Paragraph(
        "Após o front-end, a AST rebaixa para uma <b>IR SSA</b> (SIR) e, em paralelo, "
        "para um <b>TensorGraph</b> de mais alto nível usado em fusão e legalização "
        "CPU/WGPU. Autodiff é nó nativo da IR.", style_body)]
    f += bullets([
        "Lowering (AST->IR): escopo SSA, alloca para vars mutáveis, monomorfização de genéricos com name mangling e teto de 512 especializações.",
        "Autodiff: InstructionKind::AutodiffStep (contrato fechado) materializado por materialize_autodiff_steps() antes da verificação.",
        "Passes (opt-level gated): ConstantFolding (>=1), FunctionInlining + DCE (>=2), ConcurrentSpawnJoinFusion (se optimize).",
        "LoopStructureValidation roda sempre; verify_module() roda antes e depois das otimizações (verificação dupla).",
    ])
    f += [Paragraph(
        "IR carrega debug info: SourceSpan por instrução e LocalDebugInfo por local — "
        "habilita stack traces nível código-fonte.", style_caption)]
    f += [PageBreak()]

    # 11 - BACK-END ---------------------------------------------------------
    f += slide(11, "Back-end: Cranelift JIT + AOT")
    f += bullets([
        "JIT (caminho primário): JITBuilder com optimizer 'speed' (padrão é none); declara/define funções via FunctionBuilder; PHI -> block parameters do Cranelift.",
        "AOT: cranelift-object emite COFF/ELF/Mach-O; --emit-exe sintetiza shim main(argc,argv) que sobe o runtime e chama o entry point.",
        "Host calls: ponte entre código nativo JIT e Rust via ABI SpectraHostValue + registro de funções; fast-paths no_mangle para string/map/concurrent.",
        "Debug: mapas JSON de debug + DWARF/CodeView para gdb/lldb/cdb； --dump-ir imprime a IR textual.",
    ])
    f += code(
        """[12a] JIT   backend/src/codegen.rs   CodeGenerator -> JITModule -> memória
 [12b] AOT   backend/src/aot.rs       AotCodeGenerator -> .o/.obj -> linker
                                 (requer libspectra_runtime.a)
""")
    f += [PageBreak()]

    # 12 - RUNTIME ----------------------------------------------------------
    f += slide(12, "Runtime: memória híbrida, reactor, stdlib, api")
    f += bullets([
        "HybridMemory: GC tracado (Gc<T>, GcRoot<T>) para valores gerenciados + frames manuais (manual_frame_enter/exit) para scratch do JIT.",
        "Host-call ABI estável: registro process-wide (mutex); status codes SUCCESS/INVALID_ARG/NOT_FOUND/INTERNAL.",
        "stdlib registrada em runtime::initialize() via register_standard_library() + spectra_api::register() (contrato de 211 host calls tipados).",
        "Reactor async sobre mio + Waker + fila de prioridade; exposto via spectra.async.reactor.*.",
    ])
    f += [Paragraph(
        "spectra.api (211 host calls assereados em compile-time entre "
        "<font name='Courier'>runtime/src/api/mod.rs</font> e o crate "
        "<font name='Courier'>spectra-api</font>): http, server, client, json, tls, "
        "routing, middleware, db (sqlite/postgres/redis), trace, metrics, health.", style_caption)]
    f += [PageBreak()]

    # 13 - TOOLCHAIN --------------------------------------------------------
    f += slide(13, "Toolchain completo em um único binário")
    f += [Paragraph(
        "Tudo em <font name='Courier'>spectralang</font> — sem instalar formatter, "
        "linter, package manager ou test runner separados.", style_body)]
    f.append(card_table([
        ["Comando", "Propósito"],
        ["run / compile / check", "Execução JIT, compilação, type-check"],
        ["lint", "3 regras; --allow/--deny"],
        ["fmt", "Formatador (inclusive --stdin)"],
        ["repl / new", "REPL interativo; scaffolder de projeto"],
        ["package (19 subcmds)", "lock, build, test, add, publish, catalog..."],
        ["db", "Migrações de banco (apply/inspect/rollback)"],
        ["--json / --sarif", "Diagnósticos SARIF 2.1.0 p/ GitHub Code Scanning"],
    ], [55 * mm, 112 * mm]))
    f += [Spacer(1, 3 * mm)]
    f += [Paragraph(
        "LSP <font name='Courier'>spectra-lsp</font>: 14 capacidades (hover, go-to-def, "
        "rename, completion, inlay hints, semantic tokens, quickfix...). Níveis "
        "<font name='Courier'>-O0..-O3</font>.", style_body)]
    f += [PageBreak()]

    # 14 - PROCESSO PONTA A PONTA -------------------------------------------
    f += slide(14, "Processo de Compilação ponta-a-ponta: spectralang run")
    diag = flow_diagram([
        {"label": "CLI", "sub": "main.rs", "color": PRIMARY},
        {"label": "Discovery", "sub": "topo-sort", "color": PRIMARY},
        {"label": "Front-end", "sub": "lex/parse/sem", "color": SECONDARY},
        {"label": "Mid-end", "sub": "IR/opt", "color": SECONDARY},
        {"label": "Back-end", "sub": "JIT/AOT", "color": ACCENT},
        {"label": "Runtime", "sub": "init/main", "color": GREEN},
    ], cols=3, box_w=120, box_h=48, h_gap=20, v_gap=30)
    f += [diag, Spacer(1, 3 * mm)]
    f += bullets([
        "Resolução de projeto (spectra.toml) e descoberta de fontes; ProjectPlan::topological_order() ordena por imports (detecção de ciclo).",
        "Synthetic 'module <nome>;' inserido se ausente; compilação módulo a módulo na ordem topológica.",
        "CompilationPipeline::compile() (front-end) -> FullPipelineBackend::run() (mid+back) -> execute_artifacts().",
        "Runtime: initialize() + register_standard_library() + spectra_api::register(); codegen.execute_entry_point(\"main\", ir); código de saída propagado.",
    ])
    f += [Paragraph("Fonte: tools/spectra-cli/src/compiler_integration.rs, main.rs", style_caption)]
    f += [PageBreak()]

    # 15 - EVIDÊNCIAS / MATURIDADE -----------------------------------------
    f += slide(15, "Evidências & Maturidade")
    f += [Paragraph(
        "Benchmarks cross-linguagem (31 cenários × Spectra/Go/Java/Rust, 3 warmups, "
        "20 amostras, drift <=15%):", style_body)]
    f.append(card_table([
        ["Cenário", "vs Go", "vs Rust"],
        ["tensor-reduce", "1.05x (paridade)", "1.6x"],
        ["tensor-elementwise", "1.2x", "2.0x"],
        ["tensor-matmul", "1.94x", "2.5x"],
        ["cpu-hashmap", "6.0x", "6.9x"],
        ["cpu-string-build", "71.7x (gap conhecido)", "66.9x"],
    ], [55 * mm, 56 * mm, 56 * mm]))
    f += [Spacer(1, 3 * mm)]
    f += [Paragraph(
        "<b>Roadmap:</b> ~67% concluído (180/267 itens). Fases 0–22 prontas "
        "(AI/ML core maduro, async core, API foundation). Fronteira ativa: "
        "middleware/segurança (23), recursos avançados de API (24), DB (25), "
        "observabilidade (27), conformance v1.0 (28).", style_body)]
    f += [Paragraph(
        "<b>Status honesto:</b> ainda não é v1.0 estável; JIT é primário; exe "
        "standalone não totalmente integrado ponta-a-ponta; stdlib incompleta vs plano.",
        style_body)]
    f += [PageBreak()]

    # 16 - CONSIDERAÇÕES FINAIS --------------------------------------------
    f += slide(16, "Considerações Finais")
    f += [Paragraph("Por que SpectraLang merece atenção:", style_h2)]
    f += bullets([
        "Tensores e requisições HTTP são <b>tipos de primeira classe</b> — erros de forma viram erro de compilação.",
        "Treino e serving no <b>mesmo arquivo/runtime/artefato</b>.",
        "Autodiff e async são <b>recursos do compilador</b>, não bibliotecas.",
        "JIT <b>e</b> AOT pela mesma pipeline; Cranelift em todas as platforms.",
        "Toolchain completo em <b>um binário</b> com diagnósticos SARIF e LSP de 14 capacidades.",
        "Cultura <b>evidence-driven</b>: claims de performance exigem artefatos de benchmark reproduzíveis.",
    ])
    f += [Spacer(1, 4 * mm)]
    f += [HRFlowable(width="100%", thickness=1.2, color=SECONDARY, spaceAfter=6)]
    f += [Paragraph(
        "<b>Posicionamento:</b> projeto credível e disciplinado, com núcleo de IA "
        "maduro e plataforma de API em franca evolução — não uma promessa exagerada, "
        "e sim uma base de engenharia sólida para cargas de ML e serviços.", style_body)]
    f += [Spacer(1, 6 * mm)]
    f += [Paragraph("SpectraLang · CLI v0.2.6 · MIT · compilador/runtime/toolchain em Rust", style_caption)]

    return f


# --------------------------------------------------------------------------- #
# Main
# --------------------------------------------------------------------------- #
def main():
    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "spectra_architecture.pdf")
    doc = SimpleDocTemplate(
        out, pagesize=A4,
        leftMargin=18 * mm, rightMargin=18 * mm,
        topMargin=20 * mm, bottomMargin=18 * mm,
        title="SpectraLang — Arquitetura e Processo de Compilação",
        author="SpectraLang")
    doc.build(build(), onFirstPage=_on_page, onLaterPages=_on_page)
    print("PDF gerado:", out)


if __name__ == "__main__":
    main()
