#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Ebook tecnico completo: "SpectraLang -- Guia Completo da Linguagem"
Python + reportlab 4.5.x (documento A4, guia de linguagem aprofundado).

Uso:
    python presentation/spectra_ebook_pdf.py
Gera:
    presentation/spectra_ebook.pdf
"""

import os
from reportlab.lib.pagesizes import A4
from reportlab.lib.units import mm
from reportlab.lib import colors
from reportlab.lib.styles import getSampleStyleSheet, ParagraphStyle
from reportlab.lib.enums import TA_CENTER, TA_LEFT, TA_JUSTIFY
from reportlab.platypus import (
    BaseDocTemplate, PageTemplate, Frame, Paragraph, Spacer, Table,
    TableStyle, Preformatted, PageBreak, KeepTogether, HRFlowable,
)
from reportlab.platypus.tableofcontents import TableOfContents
from reportlab.graphics.shapes import Drawing, Rect, String, Line, Polygon
from reportlab.pdfbase.ttfonts import TTFont
from reportlab.pdfbase import pdfmetrics

# --------------------------------------------------------------------------- #
# Registro de fonts TrueType (suporte a acentos e Unicode)
# --------------------------------------------------------------------------- #
_FONTS_DIR = os.path.join(os.environ.get("WINDIR", "C:\\Windows"), "Fonts")
pdfmetrics.registerFont(TTFont("Calibri", os.path.join(_FONTS_DIR, "segoeui.ttf")))
pdfmetrics.registerFont(TTFont("Calibri-Bold", os.path.join(_FONTS_DIR, "segoeuib.ttf")))
pdfmetrics.registerFont(TTFont("Calibri-Italic", os.path.join(_FONTS_DIR, "segoeuii.ttf")))
pdfmetrics.registerFont(TTFont("Consolas", os.path.join(_FONTS_DIR, "consola.ttf")))
pdfmetrics.registerFontFamily("Calibri",
    normal="Calibri", bold="Calibri-Bold", italic="Calibri-Italic")

# --------------------------------------------------------------------------- #
# Cores
# --------------------------------------------------------------------------- #
PRIMARY   = colors.HexColor("#4338CA")
SECONDARY = colors.HexColor("#6D28D9")
ACCENT    = colors.HexColor("#0D9488")
DARK      = colors.HexColor("#0F172A")
INK       = colors.HexColor("#1E293B")
GREY      = colors.HexColor("#64748B")
LIGHT_BG  = colors.HexColor("#F1F5F9")
CODE_BG   = colors.HexColor("#0B1220")
CODE_FG   = colors.HexColor("#D7E3F4")
GREEN     = colors.HexColor("#059669")
WHITE     = colors.white

# --------------------------------------------------------------------------- #
# Dimensoes
# --------------------------------------------------------------------------- #
PAGE_W, PAGE_H = A4
LMARGIN = RMARGIN = 20 * mm
TMARGIN = 22 * mm
BMARGIN = 20 * mm
USABLE  = PAGE_W - LMARGIN - RMARGIN

# --------------------------------------------------------------------------- #
# Estilos
# --------------------------------------------------------------------------- #
ss = getSampleStyleSheet()

style_cover_title = ParagraphStyle(
    "CoverTitle", parent=ss["Title"], fontName="Calibri-Bold",
    fontSize=34, leading=39, textColor=PRIMARY, alignment=TA_CENTER, spaceAfter=8)
style_cover_sub = ParagraphStyle(
    "CoverSub", parent=ss["Normal"], fontName="Calibri",
    fontSize=13.5, leading=18, textColor=INK, alignment=TA_CENTER, spaceAfter=4)
style_cover_meta = ParagraphStyle(
    "CoverMeta", parent=ss["Normal"], fontName="Calibri",
    fontSize=9.5, leading=14, textColor=GREY, alignment=TA_CENTER)

style_h1 = ParagraphStyle(
    "Chapter", parent=ss["Heading1"], fontName="Calibri-Bold",
    fontSize=18, leading=22, textColor=PRIMARY, spaceBefore=4, spaceAfter=8)
style_ch_kicker = ParagraphStyle(
    "ChKicker", parent=ss["Normal"], fontName="Calibri-Bold",
    fontSize=11, leading=14, textColor=ACCENT, alignment=TA_CENTER, spaceAfter=2)
style_ch_title = ParagraphStyle(
    "ChTitle", parent=ss["Title"], fontName="Calibri-Bold",
    fontSize=26, leading=30, textColor=INK, alignment=TA_CENTER, spaceAfter=6)

style_h2 = ParagraphStyle(
    "Section", parent=ss["Heading2"], fontName="Calibri-Bold",
    fontSize=14, leading=18, textColor=SECONDARY, spaceBefore=12, spaceAfter=4)
style_h3 = ParagraphStyle(
    "SubSection", parent=ss["Heading3"], fontName="Calibri-Bold",
    fontSize=12, leading=16, textColor=INK, spaceBefore=8, spaceAfter=3)

style_body = ParagraphStyle(
    "BodyX", parent=ss["Normal"], fontName="Calibri",
    fontSize=11, leading=16, textColor=INK, spaceAfter=6, alignment=TA_JUSTIFY)
style_bullet = ParagraphStyle(
    "BulletX", parent=style_body, leftIndent=14, bulletIndent=3,
    spaceAfter=2.5, alignment=TA_LEFT)
style_caption = ParagraphStyle(
    "CapX", parent=ss["Normal"], fontName="Calibri-Italic",
    fontSize=9, leading=12, textColor=GREY, spaceBefore=2, spaceAfter=10)
style_code = ParagraphStyle(
    "CodeX", fontName="Consolas", fontSize=8.5, leading=11,
    textColor=CODE_FG, backColor=None, leftIndent=0,
    spaceBefore=0, spaceAfter=0)
style_code_num = ParagraphStyle(
    "CodeNum", fontName="Consolas", fontSize=8.5, leading=11,
    textColor=GREY, backColor=None, alignment=TA_CENTER)
style_code_label = ParagraphStyle(
    "CodeLabel", fontName="Consolas", fontSize=7.8, leading=9.5,
    textColor=WHITE, backColor=None)
style_note = ParagraphStyle(
    "NoteX", parent=ss["Normal"], fontName="Calibri",
    fontSize=10, leading=14, textColor=INK, spaceAfter=0)
style_toc1 = ParagraphStyle(
    "TOC1", parent=ss["Normal"], fontName="Calibri-Bold",
    fontSize=12, leading=20, textColor=INK)
style_toc2 = ParagraphStyle(
    "TOC2", parent=ss["Normal"], fontName="Calibri",
    fontSize=10, leading=16, textColor=GREY, leftIndent=12)


def esc(t: str) -> str:
    return (t.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;"))


# --------------------------------------------------------------------------- #
# Diagramas
# --------------------------------------------------------------------------- #
def _arrow_head(x, y, direction):
    if direction == "right":
        return Polygon([x, y - 3.2, x - 6.5, y, x, y + 3.2],
                       fillColor=GREY, strokeColor=None)
    if direction == "down":
        return Polygon([x - 3.2, y, x, y + 6.5, x + 3.2, y],
                       fillColor=GREY, strokeColor=None)
    if direction == "left":
        return Polygon([x, y - 3.2, x + 6.5, y, x, y + 3.2],
                       fillColor=GREY, strokeColor=None)
    return Polygon([x - 3.2, y, x, y - 6.5, x + 3.2, y],
                   fillColor=GREY, strokeColor=None)


def flow_diagram(items, cols=4, box_w=104, box_h=48, h_gap=16, v_gap=32,
                 color=PRIMARY):
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
        d.add(Rect(x, y, box_w, box_h, rx=7, ry=7,
                   fillColor=it.get("color", color), strokeColor=None))
        d.add(String(x + box_w / 2, y + box_h / 2 + 6, it["label"],
                     fontSize=8.6, fillColor=WHITE, textAnchor="middle",
                     fontName="Calibri-Bold"))
        if it.get("sub"):
            d.add(String(x + box_w / 2, y + box_h / 2 - 7, it["sub"],
                         fontSize=6.3, fillColor=WHITE, textAnchor="middle",
                         fontName="Calibri"))
    for idx in range(len(items) - 1):
        r, c = divmod(idx, cols)
        if c < cols - 1:
            x1 = pos[idx][0] + box_w
            x2 = pos[idx + 1][0]
            y = pos[idx][1] + box_h / 2
            d.add(Line(x1, y, x2 - 7, y, strokeColor=GREY, strokeWidth=1.3))
            d.add(_arrow_head(x2 - 7, y, "right"))
        else:
            x = pos[idx][0] + box_w / 2
            y1 = pos[idx][1]
            y2 = pos[idx + 1][1] + box_h
            d.add(Line(x, y1, x, y2 + 6, strokeColor=GREY, strokeWidth=1.3))
            d.add(_arrow_head(x, y2 + 6, "down"))
    return d


# --------------------------------------------------------------------------- #
# Doc com TOC
# --------------------------------------------------------------------------- #
BOOK_SHORT = "SpectraLang - Do Zero ao Avancado"


class EbookDoc(BaseDocTemplate):
    def afterFlowable(self, flowable):
        if isinstance(flowable, Paragraph):
            sname = flowable.style.name
            if sname == "Chapter":
                self.notify("TOCEntry", (0, flowable.getPlainText(), self.page))
            elif sname == "Section":
                self.notify("TOCEntry", (1, flowable.getPlainText(), self.page))


def _header_footer(canvas, doc):
    canvas.saveState()
    if doc.page == 1:
        canvas.restoreState()
        return
    canvas.setStrokeColor(LIGHT_BG)
    canvas.setLineWidth(0.8)
    canvas.line(LMARGIN, PAGE_H - 15 * mm, PAGE_W - RMARGIN, PAGE_H - 15 * mm)
    canvas.setFillColor(GREY)
    canvas.setFont("Calibri", 7.6)
    canvas.drawString(LMARGIN, PAGE_H - 12.6 * mm, BOOK_SHORT)
    canvas.drawRightString(PAGE_W - RMARGIN, PAGE_H - 12.6 * mm,
                           "v0.3.0 - MIT")
    canvas.line(LMARGIN, 14 * mm, PAGE_W - RMARGIN, 14 * mm)
    canvas.setFont("Calibri", 7.6)
    canvas.drawString(LMARGIN, 10 * mm, "Tutorial completo -- gerado com Python/reportlab")
    canvas.drawRightString(PAGE_W - RMARGIN, 10 * mm, "Pagina %d" % doc.page)
    canvas.restoreState()


# --------------------------------------------------------------------------- #
# Helpers de conteudo
# --------------------------------------------------------------------------- #
story = []


def chap(num, title):
    story.append(PageBreak())
    story.append(Spacer(1, 26 * mm))
    story.append(Paragraph("PARTE - CAPITULO %d" % num, style_ch_kicker))
    story.append(Paragraph(title, style_ch_title))
    story.append(HRFlowable(width="40%", thickness=2, color=ACCENT,
                            spaceBefore=6, spaceAfter=10, hAlign="CENTER"))
    story.append(Paragraph("Capitulo %d -- %s" % (num, title), style_h1))


def sec(title):
    story.append(Paragraph(title, style_h2))


def subsec(title):
    story.append(Paragraph(title, style_h3))


def p(text):
    story.append(Paragraph(text, style_body))


def bul(items):
    for i in items:
        story.append(Paragraph("* " + i, style_bullet))


def code(text, caption=None, title=None, skinny_nums=False):
    lines = text.split("\n")
    if lines and lines[-1] == "":
        lines = lines[:-1]
    nums = "\n".join("%*d" % (len(str(len(lines))), i + 1) for i in range(len(lines)))
    code_cell = Preformatted(esc(text.rstrip("\n")), style_code)
    num_cell = Preformatted(nums, style_code_num)
    nw = 8 * mm if skinny_nums else 12 * mm
    body = Table([[num_cell, code_cell]],
                 colWidths=[nw, USABLE - nw])
    body.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (0, 0), colors.HexColor("#E2E8F0")),
        ("BACKGROUND", (1, 0), (1, 0), CODE_BG),
        ("VALIGN", (0, 0), (-1, -1), "TOP"),
        ("LEFTPADDING", (0, 0), (0, 0), 3),
        ("RIGHTPADDING", (0, 0), (0, 0), 3),
        ("LEFTPADDING", (1, 0), (1, 0), 8),
        ("RIGHTPADDING", (1, 0), (1, 0), 6),
        ("TOPPADDING", (0, 0), (-1, -1), 6),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 6),
        ("LINEBEFORE", (1, 0), (1, 0), 2.2, ACCENT),
    ]))
    flow = []
    if title:
        hdr = Table([[Paragraph(esc(title), style_code_label)]],
                    colWidths=[USABLE])
        hdr.setStyle(TableStyle([
            ("BACKGROUND", (0, 0), (-1, -1), ACCENT),
            ("LEFTPADDING", (0, 0), (-1, -1), 7),
            ("TOPPADDING", (0, 0), (-1, -1), 2.5),
            ("BOTTOMPADDING", (0, 0), (-1, -1), 2.5),
        ]))
        flow.append(hdr)
    flow.append(body)
    story.append(KeepTogether(flow))
    if caption:
        story.append(Paragraph(esc(caption), style_caption))
    else:
        story.append(Spacer(1, 4 * mm))


def nota(text):
    pp = Paragraph(text, style_note)
    t = Table([[pp]], colWidths=[USABLE])
    t.setStyle(TableStyle([
        ("BACKGROUND", (0, 0), (-1, -1), LIGHT_BG),
        ("LINEBEFORE", (0, 0), (0, -1), 3, ACCENT),
        ("LEFTPADDING", (0, 0), (-1, -1), 9),
        ("RIGHTPADDING", (0, 0), (-1, -1), 9),
        ("TOPPADDING", (0, 0), (-1, -1), 6),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 6),
    ]))
    story.append(t)
    story.append(Spacer(1, 4 * mm))


def tbl(headers, rows, col_widths, hdr_color=PRIMARY):
    data = [headers] + rows
    tw = Table(data, colWidths=col_widths, repeatRows=1)
    tw.setStyle(TableStyle([
        ("FONTNAME", (0, 0), (-1, -1), "Calibri"),
        ("FONTSIZE", (0, 0), (-1, -1), 8.4),
        ("BACKGROUND", (0, 0), (-1, 0), hdr_color),
        ("TEXTCOLOR", (0, 0), (-1, 0), WHITE),
        ("FONTNAME", (0, 0), (-1, 0), "Calibri-Bold"),
        ("ROWBACKGROUNDS", (0, 1), (-1, -1), [WHITE, LIGHT_BG]),
        ("LINEBELOW", (0, 0), (-1, -1), 0.4, colors.HexColor("#CBD5E1")),
        ("TOPPADDING", (0, 0), (-1, -1), 4),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 4),
        ("LEFTPADDING", (0, 0), (-1, -1), 6),
    ]))
    story.append(tw)
    story.append(Spacer(1, 4 * mm))


# ==================== CONTEUDO ====================
def build_content():
    # Part 1: Chapters 1-5 -- Foundations
    # This file is appended to the story list during PDF generation

    # ===================== CAPA =====================
    story.append(Spacer(1, 46 * mm))
    story.append(Paragraph("SpectraLang", style_cover_title))
    story.append(Paragraph("Do Zero ao Avancado", style_cover_sub))
    story.append(Paragraph(
        "Um tutorial completo da linguagem SpectraLang -- dos fundamentos da "
        "programacao ate tipos algebricos, tensores, async/await, APIs HTTP "
        "nativas e o compilador. Escrito como um livro-texto para quem esta "
        "comecando do zero absoluto.",
        style_cover_meta))
    story.append(Spacer(1, 10 * mm))
    story.append(HRFlowable(width="50%", thickness=2, color=ACCENT, hAlign="CENTER"))
    story.append(Spacer(1, 8 * mm))
    story.append(Paragraph(
        "Linguagem compilada implementada em Rust \u00b7 JIT/AOT via Cranelift \u00b7 "
        "Memoria hibrida (GC + manual) \u00b7 Seguranca de tipos rigorosa \u00b7 "
        "Tensors, autodiff, async/await, HTTP nativo",
        style_cover_meta))
    story.append(Spacer(1, 40 * mm))
    story.append(Paragraph("CLI v0.3.0 \u00b7 Licenca MIT \u00b7 Julho 2026", style_cover_meta))

    # ===================== SUMARIO =====================
    story.append(PageBreak())
    story.append(Spacer(1, 6 * mm))
    story.append(Paragraph("Sumario", style_h1))
    toc = TableOfContents()
    toc.levelStyles = [style_toc1, style_toc2]
    story.append(toc)
    story.append(PageBreak())

    # ================================================================
    # PARTE I: FUNDAMENTOS ABSOLUTOS (Caps 1-3)
    # ================================================================

    chap(1, "O Que E Programacao -- E O Que E SpectraLang?")

    p("Um computador, no fundo, e uma maquina extraordinariamente obediente e "
      "extraordinariamente estupida. Ele executa instrucoes -- uma apos a outra, "
      "bilhoes de vezes por segundo -- sem jamais questionar o que esta fazendo. "
      "A unica coisa que ele entende sao numeros: codigos binarios que representam "
      "operacoes como 'some estes dois valores' ou 'guarde este numero na posicao "
      "de memoria 83742'. Escrever programas diretamente nessa linguagem de "
      "numeros -- chamada <i>codigo de maquina</i> -- e possivel, mas seria como "
      "tentar escrever um romance apontando para caracteres individuais numa tabela "
      "ASCII. Fazivel, mas tao lento e propenso a erros que ninguem faria.")

    p("E aqui que entra a ideia central de uma <b>linguagem de programacao</b>: "
      "ela e uma ponte entre a forma como nos, humanos, pensamos sobre problemas, "
      "e a forma como o computador executa instrucoes. Em vez de escrevermos "
      "sequencias de numeros binarios, escrevemos algo como "
      "<font name='Consolas'>let idade = 25;</font> -- uma frase que, para nos, "
      "tem significado imediato: 'crie um espaco na memoria chamado idade e "
      "coloque o valor 25 dentro dele'. Um programa especial chamado "
      "<b>compilador</b> le esse texto, verifica se ele faz sentido, e o traduz "
      "para o codigo de maquina que o processador entende. A linguagem de "
      "programacao e, portanto, um contrato: ela nos da um vocabulario e uma "
      "gramatica que podemos usar para descrever computacoes, e em troca o "
      "compilador garante que essas descricoes sejam traduzidas corretamente.")

    p("SpectraLang e uma dessas linguagens. Mas ela nao foi criada para ser "
      "'mais uma linguagem'. Ela nasceu de uma observacao concreta: no mundo "
      "real, as pessoas que treinam modelos de inteligencia artificial usam Python, "
      "e as pessoas que colocam esses modelos em producao usam Go, Rust ou Java. "
      "Existe uma fronteira invisivel entre o mundo do treinamento e o mundo do "
      "<i>serving</i>, e cruza-la significa reescrever codigo, gerenciar dois "
      "runtimes diferentes, serializar modelos e conviver com custos de integracao "
      "que nao agregam valor algum ao produto final. A tese central de SpectraLang "
      "e que essa fronteira nao deveria existir: a mesma linguagem que descreve um "
      "tensor e sua retropropagacao deveria ser capaz de descrever a requisicao "
      "HTTP que serve o modelo treinado. Treino e serving, no mesmo arquivo, no "
      "mesmo runtime, no mesmo artefato de deploy.")

    p("Para realizar essa visao, SpectraLang foi construida sobre dois pilares "
      "complementares. O primeiro e o workstream de <b>IA/ML</b>: tensors com "
      "tipagem estatica de rank, dimensoes e dispositivo; diferenciacao automatica "
      "integrada ao compilador; um framework completo de treinamento com modulos, "
      "otimizadores, datasets e dataloaders; exportacao ONNX; tokenizacao; e "
      "recuperacao aumentada por geracao (RAG). O segundo e o workstream de "
      "<b>API</b>, materializado no pacote "
      "<font name='Consolas'>spectra.api</font>: um servidor HTTP nativo com "
      "tipos de primeira classe para Request, Response, Method, Status e Header; "
      "roteamento com parametros tipados; middleware composto; serializacao JSON "
      "via derive macros; e drivers para SQLite, PostgreSQL e Redis. O resultado "
      "e uma linguagem onde voce pode escrever um modelo de ML e o endpoint que o "
      "serve no mesmo arquivo, com o mesmo sistema de tipos verificando ambos.")

    p("SpectraLang e implementada em Rust como um workspace de multiplos crates. "
      "O compilador e dividido em quatro camadas: o <b>front-end</b> (lexer, "
      "parser, analise semantica e lint), o <b>mid-end</b> (rebaixamento para "
      "uma representacao intermediaria em forma SSA, otimizacoes, grafo de "
      "tensores e autodiff), o <b>back-end</b> (geracao de codigo via Cranelift, "
      "tanto em modo JIT -- compilacao sob demanda em memoria -- quanto AOT -- "
      "arquivos objeto nativos), e o <b>runtime</b> (gerenciamento hibrido de "
      "memoria com GC de rastreamento e alocacao manual, reactor de plataforma "
      "para async/await, e a biblioteca padrao). Esta arquitetura em camadas "
      "significa que ferramentas como o LSP podem reutilizar o front-end "
      "diretamente, sem carregar Cranelift ou o runtime.")

    sec("Tres Principios de Design")

    p("Toda linguagem de programacao faz escolhas. Quando o compilador encontra "
      "uma ambiguidade -- 'este codigo poderia significar duas coisas diferentes' "
      "-- alguem precisa decidir qual interpretacao prevalece. SpectraLang segue "
      "tres principios que guiam todas essas decisoes.")

    p("O primeiro principio e <b>clareza antes de concisao</b>. Muitas linguagens "
      "permitem omitir informacoes que o compilador 'poderia deduzir'. SpectraLang "
      "prefere que voce seja explicito. Se uma funcao retorna um valor, o tipo "
      "desse retorno deve estar declarado. Se uma variavel pertence a um tipo "
      "especifico, melhor declara-lo. A economia de alguns caracteres de digitacao "
      "nunca compensa a perda de clareza quando outra pessoa -- ou voce mesmo, seis "
      "meses depois -- tenta entender o que o codigo faz.")

    p("O segundo principio e <b>seguranca de tipos rigorosa</b>. SpectraLang "
      "pertence a familia das linguagens estaticamente tipadas: o compilador "
      "conhece o tipo de cada valor <i>antes</i> de o programa rodar, e rejeita "
      "operacoes que nao fazem sentido para esses tipos. Voce nao pode somar um "
      "numero com um texto, nao pode passar uma string onde se espera um inteiro, "
      "e -- com uma unica excecao -- nao pode misturar tipos numericos diferentes "
      "sem conversao explicita. A excecao e o alargamento de "
      "<font name='Consolas'>int</font> para <font name='Consolas'>float</font>, "
      "que e seguro porque todo inteiro cabe num float sem perda de precisao para "
      "valores ate 2^53. Essa rigidez nao e burocracia: e uma garantia de que "
      "categorias inteiras de bugs simplesmente nao podem existir nos seus programas.")

    p("O terceiro principio e <b>desempenho sem cerimonia</b>. Por padrao, "
      "SpectraLang gerencia a memoria para voce usando um coletor de lixo (GC) "
      "que rastreia quais valores ainda estao em uso e libera automaticamente "
      "os que nao estao mais. Para a vasta maioria do codigo -- logica de negocio, "
      "servidores web, scripts de treinamento -- isso e exatamente o que voce quer: "
      "voce cria valores, usa-os, e o runtime cuida do resto. Mas SpectraLang "
      "tambem oferece alocacao manual para momentos em que voce precisa de controle "
      "absoluto sobre o layout de memoria, como em kernels numericos de alta "
      "performance ou integracao com bibliotecas C. Essa dualidade -- GC para "
      "conveniencia, alocacao manual para controle -- e o que chamamos de "
      "modelo de <b>memoria hibrida</b>.")

    sec("Como Este Livro Esta Organizado")

    p("Este livro foi escrito para acompanhar voce desde o primeiro contato com "
      "programacao ate o dominio dos recursos mais avancados da linguagem. "
      "Cada capitulo introduz um novo conceito com explicacoes em prosa antes de "
      "mostrar codigo -- porque entender o 'porque' e mais importante do que "
      "memorizar o 'como'. Voce pode ler os capitulos em sequencia, como um "
      "curso completo, ou pular para secoes especificas quando precisar de "
      "referencia sobre um topico. As Partes I a V cobrem os fundamentos da "
      "linguagem: variaveis, controle de fluxo, funcoes, tipos compostos e "
      "abstracoes. As Partes VI a VIII exploram topicos avancados: tratamento "
      "de erros, sistema de modulos, concorrencia com async/await, tensores e "
      "machine learning. As Partes IX e X cobrem a plataforma de API e a "
      "toolchain. Os Apendices servem como referencia rapida. Seja qual for o "
      "seu ponto de partida, o objetivo e o mesmo: que voce termine este livro "
      "nao apenas sabendo <i>como</i> escrever SpectraLang, mas entendendo "
      "<i>por que</i> a linguagem funciona da forma como funciona.")

    # ================================================================
    chap(2, "Seu Primeiro Programa")

    p("Existe uma tradicao em programacao que remonta a 1978, quando Brian "
      "Kernighan e Dennis Ritchie publicaram o livro que definiu a linguagem C. "
      "O primeiro programa de todo iniciante imprime as palavras 'hello, world' "
      "na tela. A tradicao persiste porque ela encapsula, em poucas linhas, a "
      "essencia do que significa programar: voce escreve instrucoes, o computador "
      "as executa, e algo acontece no mundo -- neste caso, texto aparece no "
      "terminal. Vamos comecar exatamente por ai.")

    p("Antes de escrever qualquer codigo, voce precisa de uma ferramenta capaz "
      "de transformar o que voce digita em algo que o computador execute. "
      "SpectraLang oferece um binario chamado <font name='Consolas'>spectralang</font> "
      "que funciona como porta de entrada para tudo. O comando mais importante "
      "para comecar e <font name='Consolas'>spectralang run</font>, que recebe um "
      "arquivo, compila-o sob demanda e executa o resultado imediatamente. Para "
      "instalar, voce precisa do Rust (versao 1.75 ou superior) instalado no seu "
      "sistema. Clone o repositorio, compile e estara pronto:")

    code("""git clone <repositorio>
    cd SpectraLang
    cargo build --release
    ./target/release/spectralang --help""", title="Instalacao")

    p("Agora crie um arquivo chamado <font name='Consolas'>hello.spectra</font> "
      "com o seguinte conteudo. Vamos examinar cada linha, cada caractere, "
      "para entender exatamente o que esta acontecendo:")

    code("""module hello;

    import { println } from std.io;

    pub fn main() -> int {
        println("Hello, World!");
        return 0;
    }""",
         "Fonte: hello.spectra. Execute com: spectralang run hello.spectra",
         "hello.spectra")

    p("A primeira linha, <font name='Consolas'>module hello;</font>, declara que "
      "este arquivo pertence ao modulo chamado <font name='Consolas'>hello</font>. "
      "Em SpectraLang, todo arquivo deve comecar com uma declaracao de modulo -- "
      "e a forma como a linguagem organiza o codigo em unidades com nome. Pense "
      "nos modulos como capitulos de um livro: cada um tem um titulo, e juntos "
      "eles formam a obra completa. O nome do modulo pode usar pontos para "
      "indicar hierarquia, como <font name='Consolas'>module app.utils;</font>, "
      "mas para nosso primeiro programa um nome simples e suficiente. A "
      "declaracao de modulo deve ser a primeira linha do arquivo -- comentarios "
      "sao permitidos antes dela.")

    p("A segunda linha, <font name='Consolas'>import { println } from std.io;</font>, "
      "e uma instrucao de importacao. Ela diz ao compilador: 'eu quero usar "
      "a funcao chamada println que vive dentro do modulo std.io'. O prefixo "
      "<font name='Consolas'>std</font> indica a biblioteca padrao da linguagem, "
      "e <font name='Consolas'>io</font> e o submodulo que contem funcoes de "
      "entrada e saida -- como imprimir texto na tela e ler o que o usuario "
      "digita. Sem esta linha de importacao, o compilador nao saberia o que "
      "significa <font name='Consolas'>println</font> e rejeitaria o programa "
      "com um erro. Importacoes sao a forma como voce declara quais ferramentas "
      "externas ao seu modulo pretende usar.")

    p("As linhas tres a seis formam o que chamamos de <b>funcao principal</b> "
      "ou <b>entry point</b>. A assinatura e "
      "<font name='Consolas'>pub fn main() -> int</font>. Vamos decodificar "
      "cada parte: <font name='Consolas'>pub</font> significa 'publico' -- "
      "esta funcao pode ser chamada de fora do modulo, o que e essencial porque "
      "o runtime precisa encontra-la para iniciar o programa. "
      "<font name='Consolas'>fn</font> e a palavra-chave que declara uma funcao. "
      "<font name='Consolas'>main</font> e o nome da funcao -- por convencao, "
      "o runtime sempre procura uma funcao com este nome para comecar a execucao. "
      "Os parenteses vazios <font name='Consolas'>()</font> indicam que a funcao "
      "nao recebe nenhum parametro. A seta <font name='Consolas'>-> int</font> "
      "declara que esta funcao retorna um valor do tipo "
      "<font name='Consolas'>int</font> (um numero inteiro). O bloco entre chaves "
      "<font name='Consolas'>{ }</font> contem o corpo da funcao.")

    p("Dentro do corpo, duas instrucoes. A primeira, "
      "<font name='Consolas'>println(\"Hello, World!\");</font>, chama a funcao "
      "<font name='Consolas'>println</font> com um argumento: a string "
      "<font name='Consolas'>\"Hello, World!\"</font>. println (do ingles "
      "'print line') faz exatamente o que seu nome sugere: imprime o texto "
      "recebido e adiciona uma quebra de linha ao final. O ponto e virgula no "
      "final e obrigatorio em SpectraLang -- ele marca o fim de uma instrucao, "
      "assim como o ponto final marca o fim de uma frase. A segunda instrucao, "
      "<font name='Consolas'>return 0;</font>, termina a funcao e devolve o "
      "valor 0 para quem a chamou (neste caso, o sistema operacional). Retornar "
      "0 e uma convencao universal em programacao que significa 'tudo ocorreu "
      "bem'; qualquer outro valor indicaria que algo deu errado.")

    p("Execute o programa com <font name='Consolas'>spectralang run hello.spectra</font> "
      "e voce vera <font name='Consolas'>Hello, World!</font> aparecer no terminal. "
      "O que aconteceu nos bastidores? O front-end do compilador leu seu texto "
      "fonte e o transformou em uma arvore sintatica (AST). O mid-end converteu "
      "essa arvore em uma representacao intermediaria (IR) em forma SSA. O "
      "back-end Cranelift gerou codigo de maquina nativo para seu processador "
      "diretamente na memoria. O runtime inicializou o coletor de lixo, registrou "
      "as funcoes da biblioteca padrao, e transferiu o controle para sua funcao "
      "<font name='Consolas'>main</font>. Tudo isso em milissegundos. Voce acabou "
      "de escrever, compilar e executar seu primeiro programa em SpectraLang.")

    sec("Regras Estruturais de Todo Arquivo")

    p("Existem algumas regras que o compilador exige e que voce seguira em todos "
      "os programas SpectraLang que escrever. Elas nao sao arbitrarias -- cada uma "
      "serve a um proposito de clareza, seguranca ou organizacao.")

    p("Primeiro, a declaracao de modulo deve ser a primeira linha de codigo do "
      "arquivo (comentarios podem aparecer antes). O nome do modulo pode usar "
      "pontos separando hierarquias. Segundo, importacoes devem vir logo apos "
      "a declaracao de modulo, antes de qualquer definicao. O compilador processa "
      "as importacoes durante a fase de resolucao semantica, e voce nao pode usar "
      "algo que ainda nao foi importado. Terceiro, a funcao "
      "<font name='Consolas'>pub fn main() -> int</font> e o ponto de entrada. "
      "O runtime a chama automaticamente; sem ela, nao ha por onde comecar. "
      "Quarto, instrucoes dentro de funcoes devem ser separadas por ponto e "
      "virgula, e blocos sao delimitados por chaves.")

    sec("Convencoes de Nomenclatura")

    p("SpectraLang adota convencoes que ajudam a distinguir visualmente diferentes "
      "categorias de nomes no codigo. Variaveis e funcoes usam "
      "<font name='Consolas'>snake_case</font> (minusculas com underscores, como "
      "<font name='Consolas'>minha_variavel</font>). Tipos nomeados -- structs, enums "
      "e traits -- usam <font name='Consolas'>PascalCase</font> (cada palavra comeca "
      "com maiuscula, como <font name='Consolas'>Ponto</font> ou "
      "<font name='Consolas'>RetanguloColorido</font>). Constantes seguem "
      "<font name='Consolas'>SCREAMING_SNAKE_CASE</font> (todas maiusculas). "
      "Modulos usam snake_case com pontos como separadores. Essas convencoes nao "
      "sao impostas pelo compilador, mas segui-las torna o codigo imediatamente "
      "mais legivel para qualquer pessoa familiarizada com a linguagem.")

    # ================================================================
    chap(3, "Variaveis e Memoria")

    p("No capitulo anterior, escrevemos um programa que imprime uma mensagem "
      "fixa. Mas programas de verdade precisam lembrar de coisas. Um servidor "
      "web precisa lembrar qual usuario fez login. Um jogo precisa lembrar a "
      "posicao do personagem. Uma rede neural precisa lembrar os pesos de cada "
      "conexao. Em programacao, 'lembrar de algo' significa armazenar um valor "
      "num local nomeado da memoria do computador. Esse local nomeado e o que "
      "chamamos de <b>variavel</b>.")

    p("Para entender variaveis, ajuda pensar na memoria do computador como um "
      "armazem gigantesco com bilhoes de gavetas numeradas. Cada gaveta (cada "
      "<i>byte</i> de memoria) tem um endereco unico. Quando voce declara uma "
      "variavel com <font name='Consolas'>let idade = 25;</font>, o compilador "
      "reserva um conjunto de gavetas contiguas (8 bytes, no caso de um inteiro "
      "de 64 bits), grava o valor 25 dentro delas, e associa o nome "
      "<font name='Consolas'>idade</font> a esse endereco. A partir desse momento, "
      "toda vez que voce escrever <font name='Consolas'>idade</font> no codigo, "
      "o compilador sabe que deve ir ate aquelas gavetas especificas e ler ou "
      "escrever o valor. Voce nunca precisa saber o numero do endereco -- esse e o "
      "trabalho do compilador. Voce so usa o nome.")

    p("Mas o compilador precisa saber mais do que apenas o endereco. Ele precisa "
      "saber o <b>tipo</b> do valor armazenado. O tipo determina tres coisas "
      "fundamentais: quantos bytes a variavel ocupa, como interpretar os bits "
      "armazenados (os mesmos 8 bytes podem representar um inteiro, um float, "
      "ou parte de uma string, dependendo do tipo), e quais operacoes sao validas "
      "(voce pode multiplicar dois inteiros, mas nao faz sentido multiplicar duas "
      "strings). Em SpectraLang, o tipo pode ser declarado explicitamente -- "
      "<font name='Consolas'>let contador: int = 0;</font> -- ou inferido pelo "
      "compilador a partir do valor inicial -- "
      "<font name='Consolas'>let nome = \"Alice\";</font> deduz que "
      "<font name='Consolas'>nome</font> e do tipo <font name='Consolas'>string</font>. "
      "Nos dois casos, uma vez definido, o tipo da variavel nunca muda.")

    p("Diferente de linguagens como Rust, que exigem que voce declare "
      "explicitamente se uma variavel pode ser modificada, SpectraLang trata "
      "toda variavel como reatribuivel por padrao. Se voce escrever "
      "<font name='Consolas'>let x = 10;</font> e depois "
      "<font name='Consolas'>x = 20;</font>, a segunda linha simplesmente "
      "substitui o valor armazenado. A palavra-chave <font name='Consolas'>mut</font> "
      "existe na gramatica e e aceita pelo parser, mas a semantica atual da "
      "linguagem nao distingue entre variaveis mutaveis e imutaveis no nivel local. "
      "Esta decisao de design prioriza a ergonomia: a maioria dos programas modifica "
      "variaveis com frequencia, e forcar anotacoes de mutabilidade em cada uma "
      "adicionaria ruido visual sem beneficio proporcional de seguranca.")

    sec("Os Tipos Primitivos")

    p("SpectraLang oferece seis tipos primitivos -- os tijolos fundamentais com "
      "os quais todos os tipos mais complexos sao construidos.")

    p("<font name='Consolas'>int</font> e o tipo para numeros inteiros com "
      "sinal, armazenado em 64 bits. Pode representar valores de aproximadamente "
      "-9 quinquilhoes a +9 quinquilhoes. Voce pode escrever literais inteiros "
      "com underscores para legibilidade: <font name='Consolas'>1_000_000</font> "
      "e um milhao. <font name='Consolas'>float</font> e o tipo para numeros com "
      "parte fracionaria, seguindo o padrao IEEE 754 de 64 bits (dupla precisao). "
      "Isso significa que ele pode representar aproximadamente 15-17 digitos "
      "significativos de precisao, mas com as mesmas limitacoes que todo float "
      "tem: numeros como 0.1 nao podem ser representados exatamente em binario. "
      "<font name='Consolas'>bool</font> e o tipo mais simples: so pode ser "
      "<font name='Consolas'>true</font> ou <font name='Consolas'>false</font>. "
      "Ocupa 1 byte e e o tipo que resulta de todas as operacoes de comparacao.")

    p("<font name='Consolas'>string</font> representa uma sequencia de caracteres "
      "Unicode codificada em UTF-8, escritas entre aspas duplas. Sequencias de "
      "escape permitem caracteres especiais: <font name='Consolas'>\\n</font> e "
      "quebra de linha, <font name='Consolas'>\\t</font> e tabulacao, "
      "<font name='Consolas'>\\\"</font> e uma aspa dupla literal. "
      "<font name='Consolas'>char</font> e o tipo para um unico caractere Unicode, "
      "escrito entre aspas simples: <font name='Consolas'>'A'</font>, "
      "<font name='Consolas'>'中'</font>. Internamente, um char e representado "
      "como um inteiro contendo o codigo Unicode do caractere. Finalmente, "
      "<font name='Consolas'>unit</font> e um tipo especial que representa "
      "'nenhum valor' -- e o que uma funcao retorna quando nao declara tipo "
      "de retorno, analogo ao <font name='Consolas'>void</font> de C ou Java.")

    p("Uma caracteristica importante de SpectraLang e sua politica de conversao "
      "de tipos. A linguagem e extremamente restritiva quanto a conversoes "
      "implicitas: a unica conversao que acontece automaticamente e "
      "<font name='Consolas'>int</font> para <font name='Consolas'>float</font>. "
      "Qualquer outra conversao deve ser feita explicitamente usando funcoes do "
      "modulo <font name='Consolas'>std.convert</font> ou o operador "
      "<font name='Consolas'>as</font>. Esta restricao pode parecer pedante no "
      "inicio, mas ela previne uma classe inteira de bugs sutis que afligem "
      "linguagens mais permissivas, onde uma conversao automatica inesperada "
      "produz resultados corretos em tipo mas errados em logica de negocio.")

    sec("Declarando e Usando Variaveis")

    code("""let x = 10;                 // int (inferido)
    let pi = 3.14;              // float
    let ativo = true;           // bool
    let nome = "Alice";         // string
    let letra = 'A';            // char

    let contador: int = 0;      // tipo explicito
    contador = contador + 1;    // reatribuicao funciona

    let arr = [10, 20, 30];
    arr[0] = 99;                // modificacao indexada OK""",
         "Declaracao com let. O tipo e inferido ou anotado explicitamente.")

    p("Observe que a palavra-chave <font name='Consolas'>let</font> introduz "
      "uma nova variavel, mesmo que ja exista outra com o mesmo nome em um "
      "escopo externo. Isso e chamado de <b>shadowing</b> (sombreamento): a "
      "nova variavel 'esconde' a anterior dentro do escopo onde foi declarada. "
      "O linter de SpectraLang tem uma regra especifica para detectar "
      "sombreamento, porque embora seja uma ferramenta util em certas situacoes, "
      "o sombreamento acidental pode causar confusao. Se voce quiser apenas "
      "modificar o valor de uma variavel existente, nao use "
      "<font name='Consolas'>let</font> novamente -- apenas atribua o novo valor: "
      "<font name='Consolas'>x = 42;</font>.")

    sec("F-Strings: Interpolacao de Texto")

    p("Imprimir valores de variaveis e uma das operacoes mais comuns em "
      "programacao -- seja para depurar um problema, exibir resultados para o "
      "usuario, ou gerar logs. SpectraLang oferece um mecanismo elegante "
      "chamado <b>f-strings</b> (formatted strings) que permite embutir "
      "expressoes diretamente dentro de uma string. A sintaxe e simples: "
      "prefixe a string com <font name='Consolas'>f</font> e coloque expressoes "
      "entre chaves <font name='Consolas'>{ }</font>. O compilador avalia cada "
      "expressao, converte o resultado para string, e o insere na posicao "
      "correspondente.")

    p("O que torna f-strings poderosas e que o conteudo entre chaves e uma "
      "expressao completa da linguagem -- voce pode colocar variaveis, operacoes "
      "aritmeticas, chamadas de funcao, ou acessos a campos de struct. O "
      "compilador valida a sintaxe e os tipos dentro das chaves como faria em "
      "qualquer outro lugar do codigo, entao voce nao precisa se preocupar com "
      "erros de formatacao que so apareceriam em tempo de execucao:")

    code("""let nome = "Alice";
    let idade = 30;
    println(f"Ola, {nome}!");                     // Ola, Alice!
    println(f"Daqui a 5 anos: {idade + 5}");      // Daqui a 5 anos: 35

    let x = 4;
    println(f"O quadrado de {x} e {x * x}");      // O quadrado de 4 e 16

    let p = Ponto { x: 10, y: 20 };
    println(f"Posicao: ({p.x}, {p.y})");          // Posicao: (10, 20)""")

    # Part 2: Chapters 4-6 -- Data, Arithmetic, and Decisions

    # ================================================================
    # PARTE II: DADOS E COMPUTACAO (Caps 4-5)
    # ================================================================

    chap(4, "Numeros e Aritmetica")

    p("A aritmetica e a forma mais fundamental de computacao. Antes de existirem "
      "interfaces graficas, antes da internet, antes mesmo dos primeiros "
      "compiladores, os computadores foram inventados para fazer contas -- "
      "trajetorias balisticas, tabelas de logaritmos, calculos de engenharia. "
      "Hoje, a aritmetica continua sendo o alicerce de tudo que um computador "
      "faz: renderizar um frame de video e aritmetica matricial; aplicar um "
      "filtro numa imagem e aritmetica sobre pixels; treinar uma rede neural "
      "sao bilhoes de multiplicacoes e somas. Entender como a aritmetica funciona "
      "em SpectraLang -- as operacoes disponiveis, as regras de tipo, as "
      "armadilhas a evitar -- e essencial antes de avancar para qualquer topico "
      "mais complexo.")

    p("SpectraLang oferece os operadores aritmeticos que voce esperaria de "
      "qualquer linguagem, com duas particularidades importantes. A primeira e "
      "a regra de tipo para divisao: quando voce divide dois inteiros, o "
      "resultado e um inteiro -- a parte fracionaria e simplesmente descartada "
      "(a operacao <i>trunca</i> em direcao a zero). Isso significa que "
      "<font name='Consolas'>10 / 3</font> resulta em 3, nao em 3.333. Para "
      "obter divisao com parte decimal, pelo menos um dos operandos deve ser "
      "float. A segunda particularidade e que o operador de modulo "
      "<font name='Consolas'>%</font> tambem funciona com floats, nao apenas "
      "com inteiros -- embora o uso mais comum seja com inteiros, onde ele "
      "retorna o resto da divisao inteira.")

    p("Existe tambem uma questao de precisao que todo programador precisa "
      "entender desde cedo: floats nao sao numeros reais. Eles sao aproximacoes "
      "com precisao finita. O numero 0.1, por exemplo, nao pode ser representado "
      "exatamente em binario (assim como 1/3 nao pode ser representado "
      "exatamente em decimal -- e 0.333... infinito). Quando voce soma 0.1 dez "
      "vezes, o resultado nao e exatamente 1.0, mas algo como 0.9999999999999999. "
      "Isso nao e um bug de SpectraLang -- e uma consequencia inevitavel da "
      "representacao IEEE 754, compartilhada por praticamente todas as linguagens "
      "modernas. Para comparacoes entre floats, evite igualdade exata "
      "<font name='Consolas'>==</font>; prefira verificar se a diferenca entre "
      "os valores e menor que uma tolerancia (um epsilon). Para calculos "
      "financeiros que exigem exatidao decimal, use inteiros representando "
      "centavos, nao floats representando reais.")

    sec("Operadores Aritmeticos")

    tbl(["Operador", "Operacao", "Tipos", "Exemplo"],
        [["+", "Adicao", "int, float, string+string", "3+4 -> 7"],
         ["-", "Subtracao", "int, float", "10-3 -> 7"],
         ["*", "Multiplicacao", "int, float", "3*4 -> 12"],
         ["/", "Divisao", "int, float", "10/3 -> 3 (int, trunca)"],
         ["%", "Modulo", "int, float", "10%3 -> 1"]],
        [26 * mm, 28 * mm, 48 * mm, 62 * mm])

    code("""let soma = 10 + 5;            // 15
    let dif = 10 - 3;             // 7
    let prod = 4 * 3;             // 12
    let quoc = 10 / 3;            // 3 (divisao inteira -- trunca)
    let resto = 10 % 3;           // 1
    let quoc_f = 10.0 / 3.0;      // 3.333... (float)
    let neg = -(5 + 2);           // -7""",
         "Operadores aritmeticos. Atencao a diferenca entre divisao int e float.")

    sec("Operadores de Comparacao e Logicos")

    p("Comparacoes sao operacoes que produzem um valor booleano "
      "(<font name='Consolas'>true</font> ou <font name='Consolas'>false</font>) "
      "a partir de dois operandos. Elas sao a base de toda tomada de decisao em "
      "programacao -- e atraves de comparacoes que um programa decide qual caminho "
      "seguir. Os seis operadores testam igualdade (<font name='Consolas'>==</font>), "
      "diferenca (<font name='Consolas'>!=</font>), e as quatro relacoes de ordem "
      "(<font name='Consolas'>&lt;</font>, <font name='Consolas'>&gt;</font>, "
      "<font name='Consolas'>&lt;=</font>, <font name='Consolas'>&gt;=</font>).")

    p("Operadores logicos combinam valores booleanos. "
      "<font name='Consolas'>&&</font> (E logico) retorna true apenas se ambos "
      "os operandos forem true. <font name='Consolas'>||</font> (OU logico) "
      "retorna true se pelo menos um for true. <font name='Consolas'>!</font> "
      "(NAO logico) inverte o valor. Uma propriedade importante e a "
      "<b>avaliacao de curto-circuito</b>: o segundo operando so e avaliado se "
      "o primeiro nao for suficiente para determinar o resultado. Em "
      "<font name='Consolas'>a && b</font>, se a for false, b nunca e avaliado. "
      "Isso nao e apenas uma otimizacao -- e uma garantia semantica que permite "
      "padroes como <font name='Consolas'>x != 0 && 100 / x > 5</font>, onde "
      "a divisao so ocorre se x for diferente de zero.")

    code("""let a = true;  let b = false;
    let e_logico = a && b;         // false (ambos precisam ser true)
    let ou_logico = a || b;        // true (pelo menos um true)
    let negacao = !a;              // false (inverte)
    let complexo = (10 > 5) && (3 < 8);  // true

    // Curto-circuito na pratica
    let divisor = 0;
    let seguro = divisor != 0 && (100 / divisor) > 10;  // false, sem divisao""")

    sec("Atribuicao Composta e Precedencia")

    p("SpectraLang suporta cinco operadores de atribuicao composta: "
      "<font name='Consolas'>+=</font>, <font name='Consolas'>-=</font>, "
      "<font name='Consolas'>*=</font>, <font name='Consolas'>/=</font>, "
      "e <font name='Consolas'>%=</font>. E importante notar que, em "
      "SpectraLang, a atribuicao <b>nao e uma expressao</b> -- ela nao retorna "
      "valor. Voce nao pode escrever <font name='Consolas'>let y = (x = 10);</font> "
      "como faria em C. Isso e uma decisao deliberada de design que evita uma "
      "classe de bugs sutis onde uma atribuicao acidental dentro de uma condicao "
      "(<font name='Consolas'>if x = 5</font> em vez de "
      "<font name='Consolas'>if x == 5</font>) passa despercebida.")

    tbl(["Nivel", "Operadores"],
        [["1 (alto)", "() f() x.y x[i]"],
         ["2", "! (unario) - (unario)"],
         ["3", "* / %"],
         ["4", "+ -"],
         ["5", "< > <= >="],
         ["6", "== !="],
         ["7", "&&"],
         ["8 (baixo)", "||"]],
        [22 * mm, 138 * mm])

    nota("Divisao entre dois <font name='Consolas'>int</font> e divisao inteira "
         "(trunca em direcao a zero). Para divisao decimal, use "
         "<font name='Consolas'>float</font> em pelo menos um dos operandos.")

    # ================================================================
    chap(5, "Texto, Strings e Entrada/Saida")

    p("Se a aritmetica e o musculo da computacao, o texto e sua pele -- a "
      "interface atraves da qual programas se comunicam com humanos. Tudo que "
      "voce le numa tela, desde o resultado de um comando ate uma pagina web "
      "completa, e texto em algum nivel de abstracao. SpectraLang trata texto "
      "como um cidadao de primeira classe: strings nao sao arrays de bytes "
      "disfarcados, mas sequencias Unicode com operacoes dedicadas e seguras.")

    p("O modulo <font name='Consolas'>std.io</font> fornece as funcoes basicas "
      "de comunicacao com o mundo exterior. A dupla mais usada e "
      "<font name='Consolas'>print</font> e <font name='Consolas'>println</font>. "
      "A diferenca entre elas e sutil mas importante: <font name='Consolas'>print</font> "
      "emite o texto e mantem o cursor na mesma linha; "
      "<font name='Consolas'>println</font> emite o texto e move o cursor para "
      "a linha seguinte. Use <font name='Consolas'>print</font> quando quiser "
      "construir uma linha em partes, e <font name='Consolas'>println</font> "
      "quando cada mensagem deve ocupar sua propria linha. Existem tambem as "
      "variantes <font name='Consolas'>eprint</font> e "
      "<font name='Consolas'>eprintln</font> que escrevem na saida de erro "
      "padrao (stderr) em vez da saida normal (stdout) -- uma distincao "
      "importante em programas serios, onde mensagens de erro nao devem "
      "contaminar a saida de dados que pode estar sendo redirecionada para "
      "um arquivo ou outro programa.")

    p("Para ler dados do usuario, use "
      "<font name='Consolas'>read_line()</font> (le uma linha completa) ou "
      "<font name='Consolas'>input(prompt)</font> (exibe um prompt e retorna "
      "o que o usuario digitar). Ambas retornam strings -- se voce precisar de "
      "um numero, use as funcoes de conversao de <font name='Consolas'>std.convert</font>. "
      "O exemplo a seguir demonstra o ciclo completo de entrada, processamento e saida:")

    code("""import std.io;
    import std.convert;

    fn main() -> int {
        print("Digite seu nome: ");
        let nome = read_line();

        print("Digite sua idade: ");
        let entrada = read_line();
        let idade = std.convert.string_to_int(entrada);

        println(f"Ola, {nome}!");
        println(f"Daqui a 10 anos voce tera {idade + 10} anos.");
        return 0;
    }""", "Programa interativo simples. Entrada, conversao, saida.")

    sec("Manipulacao de Strings com std.string")

    p("SpectraLang inclui um conjunto rico de funcoes para manipular strings "
      "no modulo <font name='Consolas'>std.string</font>. Estas funcoes cobrem "
      "desde operacoes basicas como obter o comprimento da string e verificar "
      "se ela contem uma substring, ate transformacoes como converter para "
      "maiusculas/minusculas, remover espacos das pontas, e extrair fatias. "
      "Todas as operacoes respeitam a codificacao UTF-8 e operam sobre "
      "caracteres Unicode, nao sobre bytes -- embora a funcao "
      "<font name='Consolas'>len</font> retorne o numero de bytes (nao de "
      "caracteres), o que e a convencao em linguagens de sistemas por razoes "
      "de performance.")

    p("Uma operacao particularmente util e "
      "<font name='Consolas'>substring(s, inicio, fim)</font>, que extrai uma "
      "parte da string. Os indices seguem a convencao de intervalo semiaberto: "
      "<font name='Consolas'>inicio</font> e inclusivo e "
      "<font name='Consolas'>fim</font> e exclusivo. Funcoes como "
      "<font name='Consolas'>split_first</font> e "
      "<font name='Consolas'>split_last</font> permitem dividir strings por um "
      "separador sem alocar colecoes intermediarias. Para busca, "
      "<font name='Consolas'>index_of</font> retorna a posicao (em bytes) da "
      "primeira ocorrencia, ou -1 se nao encontrar. Para substituicao, "
      "<font name='Consolas'>replace</font> substitui todas as ocorrencias de "
      "um padrao por outro.")

    tbl(["Funcao", "Assinatura", "Descricao"],
        [["len", "(string) -> int", "Numero de bytes"],
         ["contains", "(string, string) -> bool", "Contem substring?"],
         ["to_upper", "(string) -> string", "Converte ASCII para maiusculo"],
         ["to_lower", "(string) -> string", "Converte ASCII para minusculo"],
         ["trim", "(string) -> string", "Remove espacos das pontas"],
         ["starts_with", "(s, prefix) -> bool", "Comeca com prefixo?"],
         ["ends_with", "(s, suffix) -> bool", "Termina com sufixo?"],
         ["concat", "(a, b) -> string", "Concatena duas strings"],
         ["char_at", "(s, idx) -> int", "Codigo Unicode na posicao; -1 se invalido"],
         ["substring", "(s, start, end) -> string", "Extrai [start, end)"],
         ["replace", "(s, from, to) -> string", "Substitui todas as ocorrencias"],
         ["index_of", "(s, sub) -> int", "Primeira posicao; -1 se nao encontrar"],
         ["reverse_str", "(string) -> string", "Inverte a string"]],
        [36 * mm, 46 * mm, 88 * mm])

    sec("std.char -- Operacoes em Caracteres")

    p("Enquanto <font name='Consolas'>std.string</font> opera sobre strings "
      "inteiras, <font name='Consolas'>std.char</font> fornece operacoes sobre "
      "caracteres individuais. Essas funcoes recebem <font name='Consolas'>int</font> "
      "(o codigo Unicode do caractere), nao valores do tipo "
      "<font name='Consolas'>char</font>. Para usar essas funcoes com caracteres "
      "de uma string, voce primeiro obtem o codigo via "
      "<font name='Consolas'>std.string.char_at()</font>, e entao passa esse inteiro "
      "para as funcoes de <font name='Consolas'>std.char</font>. As funcoes incluem "
      "classificacao (<font name='Consolas'>is_alpha</font>, "
      "<font name='Consolas'>is_digit_char</font>, <font name='Consolas'>is_whitespace_char</font>) "
      "e transformacao (<font name='Consolas'>to_upper_char</font>, "
      "<font name='Consolas'>to_lower_char</font>).")

    # ================================================================
    # PARTE III: CONTROLE DE FLUXO (Caps 6-7)
    # ================================================================

    chap(6, "Tomando Decisoes -- if, else, e unless")

    p("Ate agora, todos os nossos programas executaram instrucoes em linha "
      "reta: primeira linha, segunda linha, terceira linha, fim. Mas programas "
      "de verdade raramente sao lineares. Um servidor web precisa responder "
      "diferentemente a requisicoes GET e POST. Um jogo precisa verificar se "
      "o jogador colidiu com um obstaculo. Um programa de linha de comando "
      "precisa validar se os argumentos fazem sentido antes de prosseguir. "
      "Em todos esses casos, o programa precisa <b>tomar decisoes</b> -- "
      "executar um bloco de codigo ou outro, dependendo de uma condicao.")

    p("O mecanismo fundamental para tomada de decisao em SpectraLang e a "
      "construcao <font name='Consolas'>if</font>. Sua estrutura e intuitiva: "
      "a palavra-chave <font name='Consolas'>if</font> e seguida por uma "
      "condicao booleana e um bloco de codigo entre chaves. Se a condicao for "
      "verdadeira, o bloco e executado. Se for falsa, ele e ignorado. "
      "Opcionalmente, voce pode encadear multiplas condicoes com "
      "<font name='Consolas'>elif</font> (a forma <font name='Consolas'>elseif</font> "
      "tambem e aceita como alias), e fornecer um bloco "
      "<font name='Consolas'>else</font> que e executado quando nenhuma das "
      "condicoes anteriores e verdadeira.")

    p("Mas <font name='Consolas'>if</font> em SpectraLang tem uma "
      "caracteristica que o distingue de muitas outras linguagens: ele e "
      "uma <b>expressao</b>, nao apenas uma instrucao. Isso significa que "
      "um bloco <font name='Consolas'>if</font> pode produzir um valor, que "
      "pode ser atribuido a uma variavel ou usado diretamente em outra "
      "expressao. Cada ramo deve retornar o mesmo tipo, e o ultimo valor de "
      "cada bloco (sem ponto e virgula) e o valor que o ramo produz. Isso "
      "elimina a necessidade do operador ternario "
      "(<font name='Consolas'>cond ? a : b</font>) presente em linguagens como "
      "C e Java -- em SpectraLang, o proprio <font name='Consolas'>if</font> "
      "cumpre esse papel de forma mais legivel e com verificacao de tipos completa.")

    p("SpectraLang tambem oferece a construcao "
      "<font name='Consolas'>unless</font>, que e o inverso logico do "
      "<font name='Consolas'>if</font>: o bloco e executado quando a condicao "
      "e <b>falsa</b>. Embora semanticamente equivalente a "
      "<font name='Consolas'>if !condicao</font>, o <font name='Consolas'>unless</font> "
      "pode tornar o codigo mais natural de ler em situacoes onde a intencao "
      "e 'faca algo a menos que esta condicao seja verdadeira' -- como em "
      "guardas de validacao: 'a menos que o usuario esteja autenticado, "
      "negue o acesso'.")

    code("""let nota = 75;
    if nota >= 90 { println("A"); }
    elif nota >= 80 { println("B"); }
    elif nota >= 70 { println("C"); }
    else { println("F"); }

    // if como expressao -- atribuindo o resultado a uma variavel
    let classificacao = if nota >= 60 { "Aprovado" } else { "Reprovado" };
    let max = if a > b { a } else { b };

    // unless: executa quando a condicao e FALSA
    let autenticado = false;
    unless autenticado {
        println("Acesso negado!");
    }""")

    # ================================================================
    chap(7, "Repetindo Coisas -- Loops")

    p("Tomar decisoes e o primeiro passo para criar programas nao-lineares. "
      "O segundo passo e <b>repetir</b> acoes. Pense em quantas tarefas de "
      "programacao envolvem repeticao: processar cada linha de um arquivo, "
      "cada registro de um banco de dados, cada frame de uma animacao, cada "
      "epoca de um treinamento. Fazer isso manualmente -- copiar e colar o "
      "mesmo codigo centenas de vezes -- nao e apenas inviavel, e impossivel "
      "quando o numero de iteracoes nao e conhecido antecipadamente. Para "
      "resolver isso, toda linguagem oferece construcoes de <b>loop</b>.")

    p("SpectraLang oferece quatro formas de loop, cada uma adequada a um "
      "cenario diferente. O <font name='Consolas'>while</font> repete "
      "enquanto uma condicao for verdadeira -- e verifica a condicao "
      "<i>antes</i> de cada iteracao, o que significa que o corpo pode "
      "nunca executar se a condicao ja for falsa no inicio. O "
      "<font name='Consolas'>do-while</font> inverte essa logica: executa o "
      "corpo primeiro e verifica a condicao depois, garantindo pelo menos "
      "uma execucao. O <font name='Consolas'>for..in</font> itera sobre "
      "intervalos ou colecoes, sendo a forma mais comum e idiomatica de "
      "loop em SpectraLang. Finalmente, o <font name='Consolas'>loop</font> "
      "cria um laco infinito que so termina quando encontra um "
      "<font name='Consolas'>break</font> explicito -- util para servidores "
      "e situacoes onde a condicao de saida e complexa demais para caber na "
      "declaracao do loop.")

    p("Duas palavras-chave controlam o fluxo dentro de qualquer loop. "
      "<font name='Consolas'>break</font> interrompe o loop imediatamente e "
      "transfere o controle para a primeira instrucao apos o bloco. "
      "<font name='Consolas'>continue</font> pula o restante da iteracao "
      "atual e avanca para a proxima verificacao da condicao. Usadas com "
      "criterio, essas construcoes permitem expressar logicas de repeticao "
      "complexas de forma clara e concisa:")

    code("""// while -- verifica antes de executar
    let i = 0;
    while i < 5 {
        println(f"i={i}");
        i = i + 1;
    }

    // do-while -- executa pelo menos uma vez
    let j = 0;
    do {
        println(f"j={j}");
        j = j + 1;
    } while j < 3;

    // for..in -- itera sobre intervalos
    for i in 0..5 { println(f"{i}"); }     // 0,1,2,3,4 (exclusivo)
    for i in 1..=5 { println(f"{i}"); }    // 1,2,3,4,5 (inclusivo)

    // loop -- infinito ate break
    let cont = 0;
    loop {
        cont = cont + 1;
        if cont >= 5 { break; }
    }

    // break e continue
    for i in 0..10 {
        if i == 3 { continue; }  // pula i=3
        if i == 7 { break; }     // sai do loop
        println(f"{i}");           // imprime 0,1,2,4,5,6
    }""", "Quatro formas de loop. for..in e a mais idiomatica para intervalos conhecidos.")

    sec("switch -- Selecao por Valor")

    p("Alem das construcoes baseadas em condicoes booleanas, SpectraLang "
      "oferece <font name='Consolas'>switch</font> para selecao baseada em "
      "valores especificos. Enquanto <font name='Consolas'>if/elif</font> "
      "funciona bem para condicoes arbitrarias, "
      "<font name='Consolas'>switch</font> e mais expressivo quando voce quer "
      "comparar uma unica variavel contra multiplos valores conhecidos. A "
      "sintaxe usa <font name='Consolas'>case valor => bloco</font> para cada "
      "caso, e exige ou um caso <font name='Consolas'>else</font> padrao ou "
      "cobertura exaustiva de todos os valores possiveis. O compilador "
      "verifica que voce nao esqueceu nenhum caso.")

    code("""let dia = 3;
    switch dia {
        case 1 => { println("Segunda"); }
        case 2 => { println("Terca"); }
        case 3 => { println("Quarta"); }
        else   => { println("Outro dia"); }
    }""")

    # Part 3: Chapters 8-11 -- Functions, Collections, Structs, Enums, Pattern Matching

    # ================================================================
    chap(8, "Funcoes -- Organizando o Codigo")

    p("Escrevemos varios programas ate agora, mas todos eles colocavam todo "
      "o codigo dentro da funcao <font name='Consolas'>main</font>. Isso "
      "funciona para exemplos pequenos, mas se torna insustentavel rapidamente. "
      "Imagine um programa com 10 mil linhas, todas dentro de uma unica "
      "funcao. Como voce encontraria o trecho que calcula impostos? Como "
      "saberia quais variaveis pertencem a qual parte da logica? Se um "
      "calculo aparece em tres lugares diferentes e voce precisa corrigi-lo, "
      "como garantiria que corrigiu todos? Funcoes sao a resposta para todos "
      "esses problemas. Elas sao o mecanismo fundamental de <b>abstracao</b> "
      "em programacao: voce da um nome a um bloco de codigo, declara quais "
      "dados ele precisa receber e qual resultado produz, e a partir dai "
      "pode usa-lo em qualquer lugar como se fosse uma operacao primitiva "
      "da linguagem.")

    p("Pense em funcoes como receitas culinarias. Uma receita de bolo tem um "
      "nome ('Bolo de Chocolate'), uma lista de ingredientes necessarios "
      "(farinha, ovos, acucar, chocolate) e um procedimento que transforma "
      "esses ingredientes em um resultado (o bolo). Em SpectraLang, uma "
      "funcao tem um nome, uma lista de <b>parametros</b> (cada um com nome "
      "e tipo), um <b>tipo de retorno</b> (o que a funcao produz), e um "
      "<b>corpo</b> (o bloco de codigo que realiza a transformacao). Assim "
      "como voce pode usar a mesma receita para fazer quantos bolos quiser, "
      "pode chamar a mesma funcao com diferentes argumentos para processar "
      "diferentes dados. A palavra-chave e <font name='Consolas'>fn</font>.")

    p("SpectraLang oferece uma conveniencia expressiva chamada "
      "<b>retorno implicito</b>: a ultima expressao do corpo (aquela que nao "
      "termina com ponto e virgula) e automaticamente retornada, sem "
      "necessidade da palavra-chave <font name='Consolas'>return</font>. Isso "
      "encoraja um estilo onde funcoes sao compostas de expressoes que fluem "
      "naturalmente para um resultado. Quando voce precisa sair da funcao "
      "antes do final -- por exemplo, ao detectar uma condicao de erro logo "
      "no inicio -- o <font name='Consolas'>return</font> explicito continua "
      "disponivel e e a ferramenta correta. Essa dualidade -- retorno implicito "
      "para o caminho feliz, retorno explicito para saidas antecipadas -- "
      "combina expressividade com controle.")

    code("""// Funcao basica: parametros com tipo, retorno declarado
    fn soma(a: int, b: int) -> int {
        return a + b;
    }

    // Sem retorno declarado: retorna unit (equivalente a void)
    fn saudacao() {
        println("Ola!");
    }

    // Retorno implicito: ultima expressao sem ponto-e-virgula
    fn eh_par(n: int) -> bool { n % 2 == 0 }
    fn dobrar(x: int) -> int { x * 2 }

    // Retorno antecipado com return explicito
    fn dividir_seguro(a: int, b: int) -> int {
        if b == 0 { return 0; }
        a / b
    }

    // if como expressao dentro de funcao
    fn maximo(a: int, b: int) -> int {
        if a > b { a } else { b }
    }""", "Funcoes com retorno explicito, implicito e antecipado.")

    sec("Visibilidade e Funcoes como Valores")

    p("Por padrao, toda funcao em SpectraLang e <b>privada</b> ao modulo "
      "onde e definida. Para exporta-la, prefixe-a com "
      "<font name='Consolas'>pub</font>. A funcao "
      "<font name='Consolas'>main</font> e sempre declarada como "
      "<font name='Consolas'>pub</font> porque o runtime precisa encontra-la. "
      "Este sistema de visibilidade se aplica a todos os itens de nivel "
      "superior: structs, enums, traits, constantes e re-exports.")

    p("Em SpectraLang, funcoes podem ser tratadas como valores: voce pode "
      "passar uma funcao como argumento para outra, armazena-la em uma "
      "variavel, ou retorna-la como resultado. O tipo de uma funcao e escrito "
      "como <font name='Consolas'>fn(Parametros) -> Retorno</font>. Alem das "
      "funcoes nomeadas, SpectraLang suporta <b>closures</b> (lambdas): "
      "funcoes anonimas definidas inline com a sintaxe "
      "<font name='Consolas'>|params| corpo</font>. Closures podem capturar "
      "variaveis do escopo onde sao definidas -- uma capacidade que funcoes "
      "nomeadas nao tem. Exploraremos closures em detalhe no Capitulo 15.")

    code("""// Funcao que recebe outra funcao como argumento
    fn aplicar(x: int, f: fn(int) -> int) -> int {
        f(x)
    }

    // Passando uma closure inline
    let resultado = aplicar(5, |x: int| x * x);  // 25

    // Closure armazenada em variavel
    let dobro = |x: int| x * 2;
    let oito = dobro(4);  // 8""")

    # ================================================================
    chap(9, "Arrays, Tuplas e Intervalos")

    p("Variaveis escalares -- um numero, uma string, um booleano -- resolvem "
      "muitos problemas, mas nao todos. Quando voce precisa trabalhar com "
      "uma lista de notas de alunos, uma tabela de precos de produtos, as "
      "coordenadas x-y de um ponto no plano, ou as dimensoes de uma matriz "
      "para algebra linear, variaveis individuais se tornam inadequadas. "
      "Precisamos de tipos que agrupem multiplos valores. SpectraLang oferece "
      "arrays e tuplas para esse fim, cada um com caracteristicas distintas.")

    p("<b>Arrays</b> sao colecoes <i>homogeneas</i>: todos os elementos "
      "devem ser do mesmo tipo. Voce cria um array com a sintaxe de "
      "colchetes: <font name='Consolas'>[1, 2, 3, 4, 5]</font>. O acesso "
      "aos elementos e por indice baseado em zero: "
      "<font name='Consolas'>arr[0]</font> e o primeiro elemento. Voce pode "
      "modificar elementos individuais: "
      "<font name='Consolas'>arr[2] = 99;</font>. Arrays tem tamanho fixo "
      "apos a criacao -- se voce precisa de uma colecao que cresce "
      "dinamicamente, use as listas do modulo "
      "<font name='Consolas'>std.collections</font>. Arrays suportam "
      "multiplas dimensoes: <font name='Consolas'>[[1,2],[3,4]]</font> e "
      "uma matriz 2x2.")

    p("<b>Tuplas</b> sao colecoes <i>heterogeneas</i> de tamanho fixo: cada "
      "posicao pode ter um tipo diferente. Voce cria uma tupla com parenteses: "
      "<font name='Consolas'>(42, \"resposta\")</font>. O acesso e por "
      "indice com notacao de ponto: <font name='Consolas'>tupla.0</font>, "
      "<font name='Consolas'>tupla.1</font>. Tuplas sao particularmente uteis "
      "quando uma funcao precisa retornar multiplos valores -- em vez de criar "
      "uma struct para um retorno unico, voce pode retornar uma tupla, e o "
      "chamador pode desestrutura-la com "
      "<font name='Consolas'>let (a, b) = funcao();</font>. "
      "<b>Intervalos</b> (ranges) completam a familia de colecoes. Criados "
      "com <font name='Consolas'>0..5</font> (exclusivo) ou "
      "<font name='Consolas'>1..=5</font> (inclusivo), sao usados principalmente "
      "com loops <font name='Consolas'>for</font>, mas tambem podem ser "
      "armazenados em variaveis e passados para funcoes.")

    code("""// Arrays: elementos do mesmo tipo, indexados por [indice]
    let nums = [1, 2, 3, 4, 5];
    let nomes = ["Alice", "Bob", "Carol"];
    let primeiro = nums[0];     // 1
    nums[4] = 99;               // modificacao indexada

    // Iteracao com for
    for i in 0..5 {
        println(f"nums[{i}] = {nums[i]}");
    }

    // Arrays multidimensionais
    let matriz = [[1,2,3], [4,5,6], [7,8,9]];
    let elem = matriz[1][2];    // 6

    // Tuplas: tipos heterogeneos, acesso por .0, .1, .2...
    let par = (42, "resposta");
    let n = par.0;              // 42
    let s = par.1;              // "resposta"

    // Desestruturacao de tupla
    let (a, b) = (10, 20);

    // Intervalos (ranges)
    let r1 = 0..5;       // exclusivo: 0,1,2,3,4
    let r2 = 1..=5;      // inclusivo: 1,2,3,4,5""",
         "Arrays, tuplas e intervalos. Cada um resolve um problema diferente de agrupamento.")

    # ================================================================
    chap(10, "Structs e Metodos")

    p("Arrays e tuplas agrupam valores, mas tem uma limitacao: os elementos "
      "sao identificados por posicao, nao por nome. Numa tupla "
      "<font name='Consolas'>(\"Alice\", 30, 1.70)</font>, voce precisa "
      "lembrar que .0 e o nome, .1 e a idade e .2 e a altura. Num programa "
      "pequeno isso funciona; num sistema com dezenas de tipos de dados "
      "diferentes, e uma receita para confusao. <b>Structs</b> resolvem "
      "esse problema permitindo que voce defina tipos com <b>campos "
      "nomeados</b>. Cada campo tem um nome e um tipo, e o compilador "
      "verifica que voce esta acessando campos que realmente existem.")

    p("Uma struct e declarada com a palavra-chave "
      "<font name='Consolas'>struct</font>, seguida pelo nome do tipo "
      "(PascalCase), e um bloco com as declaracoes dos campos. Para criar "
      "uma instancia, use a sintaxe "
      "<font name='Consolas'>NomeDaStruct { campo: valor, ... }</font>. "
      "SpectraLang suporta <b>field shorthand</b>: se uma variavel local "
      "tem o mesmo nome que um campo, voce pode omitir o "
      "<font name='Consolas'>campo:</font> e escrever apenas o nome. Structs "
      "podem ser aninhadas: um campo de uma struct pode ser outra struct, "
      "com acesso encadeado via ponto.")

    p("Alem de armazenar dados, structs podem ter <b>metodos</b>: funcoes "
      "associadas ao tipo, declaradas em blocos "
      "<font name='Consolas'>impl NomeDaStruct { ... }</font>. Metodos "
      "recebem um parametro especial chamado <b>receptor</b>, que pode ser "
      "<font name='Consolas'>self</font> (toma posse do valor), "
      "<font name='Consolas'>&self</font> (referencia imutavel), ou "
      "<font name='Consolas'>&mut self</font> (referencia mutavel). Metodos "
      "que nao recebem <font name='Consolas'>self</font> sao chamados de "
      "<b>metodos estaticos</b> e sao chamados com "
      "<font name='Consolas'>NomeDaStruct::metodo()</font>. Metodos de "
      "instancia sao chamados com notacao de ponto: "
      "<font name='Consolas'>valor.metodo()</font>.")

    p("O design de metodos suporta <b>encadeamento</b> (method chaining): "
      "se um metodo retorna um valor do mesmo tipo, voce pode chamar outro "
      "metodo sobre o resultado, formando pipelines expressivos como "
      "<font name='Consolas'>ponto.mover(1,2).escalar(3).para_string()</font>. "
      "Cada chamada produz um novo valor, que alimenta a proxima chamada. "
      "Esse estilo resulta em codigo que descreve transformacoes de dados "
      "como uma sequencia de passos nomeados.")

    code("""// Declaracao de struct
    struct Pessoa {
        nome: string,
        idade: int,
        altura: float,
        ativo: bool
    }

    // Instanciacao -- note o field shorthand
    let nome = "Alice";
    let idade = 30;
    let p = Pessoa { nome, idade, altura: 1.72, ativo: true };

    // Struct aninhada
    struct Endereco { rua: string, numero: int, cidade: string }
    struct Cliente { nome: string, endereco: Endereco }
    let c = Cliente {
        nome: "Bob",
        endereco: Endereco { rua: "Av. Principal", numero: 100, cidade: "Sao Paulo" }
    };
    let cidade = c.endereco.cidade;  // acesso aninhado""",
         "Declaracao, instanciacao e acesso a campos de structs.")

    code("""// Bloco impl com metodos estaticos e de instancia
    impl Pessoa {
        // Metodo estatico (construtor): nao recebe self
        fn nova(nome: string, idade: int) -> Pessoa {
            Pessoa { nome, idade, altura: 0.0, ativo: true }
        }

        // Metodo de instancia com &self (referencia imutavel)
        fn descrever(&self) -> string {
            f"{self.nome}, {self.idade} anos"
        }

        // Metodo que retorna nova instancia modificada
        fn envelhecer(&self, anos: int) -> Pessoa {
            Pessoa { idade: self.idade + anos, ..self }
        }

        // Metodo com &mut self (referencia mutavel) -- modifica in-place
        fn ativar(&mut self) {
            self.ativo = true;
        }
    }

    // Uso com encadeamento
    let alice = Pessoa::nova("Alice", 30);
    let mais_velha = alice.envelhecer(10).envelhecer(5);  // method chaining
    println(alice.descrever());""",
         "Metodos: estaticos, de instancia (&self), mutaveis (&mut self), encadeamento.")

    # ================================================================
    chap(11, "Enums e Pattern Matching")

    p("Structs modelam a conjuncao 'E': uma Pessoa tem um nome <b>E</b> "
      "uma idade <b>E</b> uma altura. Mas muitos dominios envolvem disjuncao "
      "'OU': um resultado pode ser sucesso <b>OU</b> falha; uma forma "
      "geometrica pode ser um circulo <b>OU</b> um retangulo. Modelar essas "
      "situacoes com structs e possivel mas fragil -- voce acabaria com flags "
      "booleanas e campos opcionais, e o compilador nao poderia ajudar a "
      "garantir que voce tratou todos os casos. <b>Enums</b> (tipos algebricos "
      "de soma) sao a ferramenta correta para modelar disjuncoes de forma "
      "segura e explicita.")

    p("Um enum declara um conjunto finito de <b>variantes</b>, cada uma das "
      "quais pode opcionalmente carregar dados. A variante mais simples e a "
      "<b>unitaria</b> -- apenas um nome, sem dados associados. "
      "<font name='Consolas'>enum Cor { Vermelho, Verde, Azul }</font> "
      "define tres valores possiveis. Variantes tambem podem carregar dados "
      "em forma de tupla (<font name='Consolas'>Mover(int, int)</font>) ou "
      "struct (<font name='Consolas'>Circulo { raio: float }</font>). Para "
      "usar o valor dentro de uma variante, voce precisa fazer "
      "<b>pattern matching</b>: o compilador forca voce a considerar cada "
      "variante possivel, garantindo que nenhum caso seja esquecido.")

    p("A construcao principal de pattern matching e "
      "<font name='Consolas'>match</font>. O compilador realiza duas "
      "verificacoes cruciais: <b>exaustividade</b> (todo valor possivel deve "
      "ser coberto por pelo menos um padrao) e <b>utilidade de padroes</b> "
      "(se um padrao nunca pode ser alcancado, um aviso e emitido). "
      "SpectraLang oferece padroes literais, de identificador, de enum, "
      "OR-patterns, e o curinga <font name='Consolas'>_</font>. Duas "
      "construcoes complementares simplificam casos comuns: "
      "<font name='Consolas'>if let padrao = expressao { ... }</font> "
      "executa o bloco apenas se o padrao casar, e "
      "<font name='Consolas'>while let padrao = expressao { ... }</font> "
      "combina loop com matching -- util para processar itens de uma fila "
      "ate que ela se esgote.")

    code("""// Enums com variantes unitarias, de tupla e de struct
    enum Cor { Vermelho, Verde, Azul }
    enum Mensagem { Sair, Mover(int, int), Texto(string) }
    enum Forma {
        Circulo { raio: float },
        Retangulo { largura: float, altura: float },
        Ponto
    }

    // match com padroes literais
    let x = 5;
    let resultado = match x {
        1 => "um", 2 => "dois", 3 => "tres",
        4 => "quatro", 5 => "cinco",
        _ => "outro"
    };

    // match com padroes de enum
    match msg {
        Mensagem::Sair => println("Saindo"),
        Mensagem::Mover(x, y) => println(f"Mover para ({x},{y})"),
        Mensagem::Texto(t) => println(t)
    }

    // OR-pattern
    enum Token { Plus, Minus, Number(int) }
    fn peso(token: Token) -> int {
        match token {
            Token::Number(value) => value,
            Token::Plus | Token::Minus => 1
        }
    }

    // if let e while let
    if let Opcao::Algum(valor) = talvez {
        println(f"Tenho um valor: {valor}");
    }
    while let Opcao::Algum(item) = fila.pop() {
        processar(item);
    }

    // Metodos em enums
    impl Forma {
        fn area(&self) -> float {
            match self {
                Forma::Circulo { raio } => 3.14159 * raio * raio,
                Forma::Retangulo { largura, altura } => largura * altura,
                Forma::Ponto => 0.0
            }
        }
    }""",
         "Pattern matching com match, if let, while let. Enums com metodos.")

    # Part 4: Chapters 12-16 -- Generics, Traits, Closures, Error Handling

    # ================================================================
    # PARTE V: ABSTRACOES E POLIMORFISMO (Caps 12-15)
    # ================================================================

    chap(12, "Generics -- Codigo Que Funciona com Qualquer Tipo")

    p("Suponha que voce precise escrever uma funcao que retorna o primeiro "
      "elemento de um array. Para arrays de inteiros, voce escreveria "
      "<font name='Consolas'>fn primeiro_int(arr: [int]) -> int { arr[0] }</font>. "
      "Para arrays de strings, outra funcao identica. Para arrays de booleanos, "
      "mais uma. A logica e sempre a mesma -- 'retorne o elemento na posicao "
      "zero' -- mas o sistema de tipos exige uma funcao separada para cada "
      "tipo de elemento. Isso viola um principio fundamental da engenharia "
      "de software: <b>DRY (Don't Repeat Yourself)</b>. Para cada nova versao "
      "da mesma logica, voce introduz uma oportunidade de erro e um ponto de "
      "manutencao adicional.")

    p("<b>Generics</b> resolvem esse problema permitindo que voce escreva "
      "codigo parametrizado por tipos. Em vez de comprometer-se com um tipo "
      "especifico, voce declara um <b>parametro de tipo</b> -- por convencao, "
      "uma letra maiuscula curta como <font name='Consolas'>T</font> -- e "
      "escreve o codigo em termos desse parametro. O compilador entao "
      "<b>monomorfiza</b> o codigo: para cada combinacao concreta de tipos "
      "com que a funcao generica e usada, ele gera uma versao especializada. "
      "Se voce chamar <font name='Consolas'>primeiro&lt;int&gt;</font> e "
      "<font name='Consolas'>primeiro&lt;string&gt;</font>, duas versoes "
      "distintas sao geradas, cada uma otimizada para seu tipo concreto. "
      "O resultado e codigo generico no fonte e codigo especializado no "
      "binario -- o melhor dos dois mundos.")

    p("Generics nao se limitam a funcoes. Structs, enums, traits e blocos "
      "<font name='Consolas'>impl</font> podem ser parametrizados. Uma struct "
      "generica como <font name='Consolas'>struct Par&lt;T&gt; { primeiro: T, "
      "segundo: T }</font> pode ser instanciada com qualquer tipo. Isso "
      "permite criar estruturas de dados reutilizaveis -- pilhas, filas, "
      "arvores -- que funcionam com qualquer tipo de elemento. A biblioteca "
      "padrao de SpectraLang faz uso extensivo de generics: "
      "<font name='Consolas'>Option&lt;T&gt;</font>, "
      "<font name='Consolas'>Result&lt;T, E&gt;</font>, e as listas de "
      "<font name='Consolas'>std.collections</font> sao todos genericos.")

    p("As vezes, porem, codigo generico puro e generico demais. Se voce "
      "escrever <font name='Consolas'>fn ordenar&lt;T&gt;(arr: [T]) { ... }</font>, "
      "o compilador nao tem como saber se T suporta comparacao. Para resolver "
      "isso, SpectraLang permite <b>trait bounds</b>: voce restringe o "
      "parametro de tipo a tipos que implementam um determinado trait. "
      "<font name='Consolas'>fn ordenar&lt;T: Comparavel&gt;(arr: [T])</font> "
      "diz: 'T pode ser qualquer tipo, desde que implemente Comparavel'. "
      "Com essa restricao, o compilador sabe que &lt; e valido para T. "
      "Multiplos bounds sao separados por <font name='Consolas'>+</font>: "
      "<font name='Consolas'>T: Exibivel + Comparavel</font>.")

    code("""// Funcao generica -- T e um parametro de tipo
    fn primeiro<T>(arr: [T]) -> T { arr[0] }
    fn trocar<T>(a: T, b: T) -> (T, T) { (b, a) }

    // Chamadas: o tipo e inferido pelo argumento
    let i = primeiro([1, 2, 3]);        // T = int
    let s = primeiro(["a", "b", "c"]);  // T = string

    // Struct generica
    struct Pilha<T> {
        dados: [T],
        tamanho: int
    }

    impl Pilha<T> {
        fn nova() -> Pilha<T> {
            Pilha { dados: [], tamanho: 0 }
        }
        fn topo(&self) -> T {
            self.dados[self.tamanho - 1]
        }
    }

    // Com trait bound -- restringe T a tipos comparaveis
    fn ordenar<T: Comparavel>(arr: [T], n: int) { /* ... */ }
    fn exibir_e_comparar<T: Exibivel + Comparavel>(a: T, b: T) { /* ... */ }""",
         "Funcoes e structs genericas. Trait bounds restringem os tipos aceitos.")

    # ================================================================
    chap(13, "Traits -- Definindo Comportamento Compartilhado")

    p("Generics resolvem o problema de escrever codigo que funciona com "
      "multiplos tipos. Mas ha um segundo problema, igualmente importante: "
      "como garantir que tipos diferentes possam ser usados de forma "
      "intercambiavel quando compartilham um comportamento comum? Suponha "
      "que voce queira escrever uma funcao <font name='Consolas'>desenhar()</font> "
      "que funcione com circulos, retangulos e triangulos -- formas diferentes, "
      "mas que todas podem ser desenhadas. Se voce fizer tres funcoes separadas, "
      "perde a capacidade de tratar todas as formas uniformemente. A resposta "
      "sao <b>traits</b>.")

    p("Um trait e uma declaracao de contrato. Ele especifica um conjunto de "
      "assinaturas de funcoes que um tipo deve implementar para satisfazer o "
      "trait. A declaracao usa a palavra-chave <font name='Consolas'>trait</font>: "
      "<font name='Consolas'>trait Desenhavel { fn desenhar(&self) -> string; }</font>. "
      "Isso diz: 'qualquer tipo que afirme ser Desenhavel deve fornecer uma "
      "funcao desenhar'. Um tipo concreto implementa o trait com um bloco "
      "<font name='Consolas'>impl Trait for Tipo { ... }</font>. A partir "
      "desse momento, o tipo satisfaz o contrato, e qualquer codigo que "
      "espere um <font name='Consolas'>Desenhavel</font> pode receber o tipo.")

    p("Traits podem incluir <b>implementacoes padrao</b>: metodos cujo corpo "
      "e fornecido na propria declaracao do trait, e que os implementadores "
      "podem opcionalmente sobrescrever. Traits tambem suportam <b>heranca</b>: "
      "um trait pode exigir que o tipo implemente outro trait como "
      "pre-requisito. <font name='Consolas'>trait Animado: Exibivel { ... }</font> "
      "significa: para implementar Animado, o tipo precisa primeiro "
      "implementar Exibivel. Isso cria hierarquias de contratos onde traits "
      "mais especificos estendem traits mais gerais. O sistema de traits de "
      "SpectraLang e inspirado no de Rust, mas simplificado. O foco esta em "
      "permitir polimorfismo parametrico via generics com trait bounds como o "
      "mecanismo principal de abstracao.")

    code("""// Declaracao de trait
    trait Exibivel {
        fn exibir(&self) -> string;
    }

    // Trait com implementacao padrao
    trait Saudavel {
        fn saudar(&self) -> string;
        fn saudar_alto(&self) -> string {
            f"OLA! {self.saudar()}"   // usa saudar() e converte
        }
    }

    // Heranca de trait: Animado exige Exibivel como pre-requisito
    trait Animado: Exibivel {
        fn mover(&self) -> string;
    }

    // Implementando trait para um tipo concreto
    impl Exibivel for Pessoa {
        fn exibir(&self) -> string {
            f"{self.nome} (idade: {self.idade})"
        }
    }

    // Multiplos trait bounds com +
    fn processar<T: Exibivel + Saudavel>(item: T) -> string {
        f"{item.exibir()} diz: {item.saudar()}"
    }""",
         "Traits definem contratos. Tipos implementam traits. Generics com bounds usam traits como restricoes.")

    # ================================================================
    chap(14, "Closures e Funcoes de Alta Ordem")

    p("No Capitulo 8, vimos que funcoes podem ser passadas como argumentos "
      "e retornadas como valores. Agora vamos explorar essa ideia em "
      "profundidade, comecando pelas <b>closures</b> -- funcoes anonimas "
      "que podem ser definidas inline, no meio de uma expressao, e que "
      "tem a capacidade especial de <b>capturar variaveis</b> do escopo "
      "onde sao criadas.")

    p("A sintaxe de uma closure e compacta: barras verticais delimitam os "
      "parametros, seguidos pelo corpo. "
      "<font name='Consolas'>|x: int| x * 2</font> define uma closure que "
      "recebe um inteiro e retorna seu dobro. Se o corpo for uma unica "
      "expressao, nao sao necessarias chaves. Se for um bloco com multiplas "
      "instrucoes, use chaves. O tipo de uma closure e "
      "<font name='Consolas'>fn(T) -> R</font> -- o mesmo tipo de uma funcao "
      "nomeada, o que significa que closures podem ser usadas em qualquer "
      "lugar que aceite um <font name='Consolas'>fn(P) -> R</font>.")

    p("O que distingue closures de funcoes nomeadas e a <b>captura de "
      "ambiente</b>. Uma closure definida dentro de uma funcao pode "
      "referenciar variaveis do escopo externo. Por exemplo: "
      "<font name='Consolas'>let delta = 10; let ajustar = |x: int| x + delta;</font>. "
      "A closure armazena o valor de <font name='Consolas'>delta</font> no "
      "momento em que e criada e o utiliza toda vez que e chamada. Em "
      "SpectraLang, a captura e <b>por valor</b> (a closure recebe uma "
      "copia do valor). Isso significa que modificar a variavel original "
      "depois de criar a closure nao afeta o comportamento da closure.")

    p("Este padrao -- criar uma closure que captura dados e retorna-la -- "
      "e extremamente poderoso. Ele permite construir comportamento "
      "parametrizado sem criar structs explicitas. Considere: "
      "<font name='Consolas'>fn criar_multiplicador(fator: int) -> fn(int) -> int "
      "{ |x: int| x * fator }</font>. Esta funcao recebe um fator e "
      "retorna uma closure que multiplica qualquer numero por esse fator. "
      "Voce pode criar <font name='Consolas'>let dobrar = criar_multiplicador(2);</font> "
      "e <font name='Consolas'>let triplicar = criar_multiplicador(3);</font> "
      "-- duas funcoes diferentes, cada uma com seu proprio fator capturado:")

    code("""// Closures basicas
    let dobro = |x: int| x * 2;
    let soma = |a: int, b: int| a + b;
    let quarenta_e_dois = || 42;

    // Closure com bloco
    let valor_abs = |x: int| {
        if x < 0 { x * -1 } else { x }
    };

    // Passando closure como argumento
    fn aplicar(x: int, f: fn(int) -> int) -> int { f(x) }
    let r = aplicar(5, |n: int| n * n);  // 25

    // Retornando closure que captura o ambiente
    fn criar_multiplicador(fator: int) -> fn(int) -> int {
        |x: int| x * fator    // 'fator' e capturado por valor
    }

    let triplicar = criar_multiplicador(3);
    let resultado = triplicar(10);  // 30""",
         "Closures capturam variaveis do escopo externo por valor.")

    # ================================================================
    # PARTE VI: TRATAMENTO DE ERROS (Cap 15)
    # ================================================================

    chap(15, "Option, Result, e o Operador ?")

    p("Dois problemas sao tao universais em programacao que merecem tipos "
      "dedicados na propria linguagem. O primeiro: uma operacao que pode "
      "ou nao produzir um valor -- como buscar um elemento num array pelo "
      "indice, onde o indice pode estar fora dos limites. O segundo: uma "
      "operacao que pode ter sucesso ou falhar -- como abrir um arquivo que "
      "pode nao existir, ou analisar uma string como numero quando ela pode "
      "nao conter um numero valido. Em muitas linguagens, esses problemas "
      "sao resolvidos com valores sentinela (null, -1) ou excecoes (try/catch). "
      "SpectraLang adota uma abordagem diferente e mais segura: representa "
      "essas situacoes como <b>tipos</b> que o compilador forca voce a "
      "tratar explicitamente.")

    p("<font name='Consolas'>Option&lt;T&gt;</font> resolve o primeiro "
      "problema. E um enum com duas variantes: "
      "<font name='Consolas'>Option::Some(valor)</font> significa 'aqui esta "
      "o valor', e <font name='Consolas'>Option::None</font> significa "
      "'nao ha valor'. Quando uma funcao retorna "
      "<font name='Consolas'>Option&lt;int&gt;</font>, voce nao pode "
      "simplesmente usar o resultado como se fosse um inteiro -- o compilador "
      "exige que voce verifique se e Some ou None antes de acessar o valor. "
      "Isso elimina o infame 'null pointer exception' da classe de erros "
      "possiveis: em SpectraLang, voce nao pode esquecer de tratar a "
      "ausencia de valor porque o sistema de tipos nao deixa.")

    p("<font name='Consolas'>Result&lt;T, E&gt;</font> generaliza Option "
      "para casos onde o que interessa nao e apenas a ausencia de valor, "
      "mas o <i>motivo</i> da falha. Tem duas variantes: "
      "<font name='Consolas'>Result::Ok(valor)</font> para sucesso e "
      "<font name='Consolas'>Result::Err(erro)</font> para falha. O tipo "
      "<font name='Consolas'>E</font> pode ser qualquer coisa -- uma string "
      "com a mensagem de erro, um enum que descreve diferentes modos de "
      "falha, ou ate mesmo um inteiro com um codigo de erro. Isso permite "
      "que funcoes comuniquem nao apenas que falharam, mas por que falharam.")

    p("O operador <font name='Consolas'>?</font> e o coroamento do sistema "
      "de tratamento de erros. Ele desembrulha um <font name='Consolas'>Result</font> "
      "ou <font name='Consolas'>Option</font>: se for Ok/Some, extrai o valor "
      "e continua; se for Err/None, retorna antecipadamente da funcao com "
      "o erro. Isso transforma codigo que seria uma cascata de "
      "<font name='Consolas'>match</font> aninhados em uma sequencia linear "
      "elegante. A funcao que usa <font name='Consolas'>?</font> deve ter "
      "retorno compativel com o tipo propagado. O modulo "
      "<font name='Consolas'>std.option</font> e "
      "<font name='Consolas'>std.result</font> fornecem funcoes auxiliares "
      "como <font name='Consolas'>is_some</font>, <font name='Consolas'>unwrap</font>, "
      "e <font name='Consolas'>unwrap_or</font> para casos onde um match "
      "seria excessivamente verboso.")

    code("""// Option<T>: valor que pode ou nao existir
    fn dividir_seguro(a: int, b: int) -> Option<int> {
        if b == 0 { return Option::None; }
        Option::Some(a / b)
    }

    match dividir_seguro(10, 2) {
        Option::Some(v) => println(f"Resultado: {v}"),
        Option::None    => println("Divisao por zero!")
    }

    // Result<T, E>: sucesso ou falha com motivo
    fn dividir(a: int, b: int) -> Result<int, string> {
        if b == 0 { return Result::Err("divisao por zero"); }
        Result::Ok(a / b)
    }

    match dividir(10, 2) {
        Result::Ok(n) => println(f"Sucesso: {n}"),
        Result::Err(msg) => println(f"Erro: {msg}")
    }

    // Operador ? -- desembrulha ou retorna erro
    fn processar(entrada: string) -> Result<int, string> {
        let n = analisar_inteiro(entrada)?;
        Result::Ok(n * 2)
    }

    // Encadeamento elegante com ?
    fn pipeline(a: string, b: string) -> Result<int, string> {
        let x = analisar_inteiro(a)?;
        let y = analisar_inteiro(b)?;
        let r = dividir(x, y)?;
        Result::Ok(r)
    }

    // Stdlib helpers
    import std.option as opt;
    let tem = opt.is_some(Option::Some(42));           // true
    let val = opt.option_unwrap(Option::Some(42));      // 42
    let def = opt.option_unwrap_or(Option::None, 0);    // 0

    import std.result as res;
    let ok = res.is_ok(Result::Ok(100));                // true
    let v = res.result_unwrap(Result::Ok(42));           // 42
    let d = res.result_unwrap_or(Result::Err("e"), 0);   // 0""",
         "Option para valores opcionais. Result para operacoes falhaveis. ? para propagacao.")

    # ================================================================
    chap(16, "Sistema de Modulos e Visibilidade")

    p("Todo programa SpectraLang que escrevemos ate agora comecou com uma "
      "declaracao <font name='Consolas'>module</font> seguida por importacoes. "
      "E hora de entender exatamente como o sistema de modulos funciona. "
      "Modulos sao a unidade fundamental de organizacao de codigo em "
      "SpectraLang: cada arquivo e um modulo, e modulos podem ser aninhados "
      "em uma hierarquia que espelha a estrutura de diretorios. O sistema de "
      "modulos serve a tres propositos: organizar codigo em unidades logicas, "
      "controlar a visibilidade (o que e publico e o que e privado), e "
      "gerenciar dependencias entre partes do programa.")

    p("A importacao oferece quatro formas. A <b>qualificada</b> "
      "(<font name='Consolas'>import std.io;</font>) importa o modulo e "
      "requer que voce use o caminho completo para acessar seus itens "
      "(<font name='Consolas'>std.io.println(...)</font>). Com <b>alias</b> "
      "(<font name='Consolas'>import std.math as math;</font>) voce encurta "
      "o prefixo. A importacao <b>por nome</b> "
      "(<font name='Consolas'>import { println } from std.io;</font>) traz "
      "itens especificos para o escopo local. E o <b>re-export</b> "
      "(<font name='Consolas'>pub import { ... } from ...;</font>) torna "
      "itens importados disponiveis para quem importar o seu modulo. "
      "A escolha entre qualificada e por nome e questao de estilo: importacao "
      "qualificada evita conflitos de nome e deixa claro de onde cada simbolo "
      "vem; importacao por nome reduz verbosidade.")

    p("SpectraLang tem tres niveis de visibilidade. O padrao (sem "
      "modificador) e <b>privado</b>: apenas o modulo atual pode acessar. "
      "<font name='Consolas'>pub</font> torna o item <b>publico</b>, acessivel "
      "de qualquer modulo. <font name='Consolas'>internal</font> restringe o "
      "acesso ao <b>pacote</b> -- util para expor APIs entre modulos do mesmo "
      "projeto sem torna-las parte da interface publica permanente. O "
      "compilador impoe regras de consistencia: funcoes "
      "<font name='Consolas'>pub</font> nao podem expor tipos privados em "
      "suas assinaturas, structs <font name='Consolas'>pub</font> nao podem "
      "ter campos de tipos privados, e assim por diante. Essas regras "
      "garantem que o encapsulamento nao possa ser violado acidentalmente.")

    code("""// Formas de importacao
    import std.io;                        // qualificada
    import std.math as math;              // alias
    import { println } from std.io;       // por nome
    pub import { println } from std.io;   // re-export

    // Visibilidade de 3 niveis
    module minha.biblioteca;
    pub struct Ponto { pub x: int, pub y: int }  // campos publicos
    internal fn util() -> int { 42 }              // pacote
    fn helper() -> int { util() }                  // privada""")

    p("A biblioteca padrao e organizada em modulos funcionais. "
      "<font name='Consolas'>std.io</font> para entrada e saida, "
      "<font name='Consolas'>std.string</font> para manipulacao de texto, "
      "<font name='Consolas'>std.math</font> para funcoes matematicas, "
      "<font name='Consolas'>std.convert</font> para conversao de tipos, "
      "<font name='Consolas'>std.collections</font> para listas dinamicas, "
      "<font name='Consolas'>std.random</font> para numeros aleatorios, "
      "<font name='Consolas'>std.fs</font> para sistema de arquivos, "
      "<font name='Consolas'>std.env</font> para variaveis de ambiente, "
      "<font name='Consolas'>std.time</font> para tempo e duracao, "
      "<font name='Consolas'>std.tensor</font> para tensores, "
      "<font name='Consolas'>std.ml</font> para machine learning, "
      "<font name='Consolas'>std.concurrent</font> para concorrencia, "
      "e <font name='Consolas'>std.serve</font> para serving e guardrails. "
      "Cada um desses modulos sera explorado nas Partes VII a IX.")

    # Part 5: Chapters 17-22 -- Stdlib, Async/Await, Tensors, ML, API Platform

    # ================================================================
    # PARTE VII: A BIBLIOTECA PADRAO (Caps 17-19)
    # ================================================================

    chap(17, "std.io, std.string, std.char, std.math, std.convert")

    sec("std.io -- Entrada e Saida")
    code("""import std.io;

    println("Ola, mundo!");     // imprime com nova linha
    print("Sem ");              // sem nova linha
    print("nova linha");
    println("!");               // "Sem nova linha!"

    eprint("Erro: ");           // stderr sem nova linha
    eprintln("arquivo nao encontrado");  // stderr com nova linha

    flush();                    // esvazia buffer de saida
    let nome = input("Digite seu nome: ");  // prompt + leitura
    let linha = read_line();    // le uma linha da entrada padrao""")

    sec("std.string -- Manipulacao de Strings")
    p("O modulo <font name='Consolas'>std.string</font> oferece funcoes "
      "para inspecao, transformacao e extracao de strings. As funcoes operam "
      "sobre indices de bytes na codificacao UTF-8 -- isso importa quando "
      "voce trabalha com caracteres nao-ASCII que ocupam multiplos bytes.")

    tbl(["Funcao", "Assinatura", "Descricao"],
        [["len", "(string) -> int", "Numero de bytes"],
         ["contains", "(string, string) -> bool", "Contem substring?"],
         ["to_upper", "(string) -> string", "Converte ASCII para maiusculo"],
         ["to_lower", "(string) -> string", "Converte ASCII para minusculo"],
         ["trim", "(string) -> string", "Remove whitespace das pontas"],
         ["starts_with", "(s, prefix) -> bool", "Comeca com prefixo?"],
         ["ends_with", "(s, suffix) -> bool", "Termina com sufixo?"],
         ["concat", "(a, b) -> string", "Concatena duas strings"],
         ["repeat_str", "(s, n) -> string", "Repete a string n vezes"],
         ["char_at", "(s, idx) -> int", "Codigo Unicode na posicao; -1 se out-of-bounds"],
         ["substring", "(s, start, end) -> string", "Extrai substring [start, end)"],
         ["replace", "(s, from, to) -> string", "Substitui todas as ocorrencias"],
         ["index_of", "(s, sub) -> int", "Posicao da primeira ocorrencia; -1 se nao encontrada"],
         ["split_first", "(s, sep) -> string", "Parte antes do primeiro separador"],
         ["split_last", "(s, sep) -> string", "Parte apos o ultimo separador"],
         ["count_occurrences", "(s, sub) -> int", "Conta ocorrencias da substring"],
         ["is_empty", "(string) -> bool", "String vazia?"],
         ["pad_left", "(s, w, ch) -> string", "Preenche a esquerda com char"],
         ["pad_right", "(s, w, ch) -> string", "Preenche a direita com char"],
         ["reverse_str", "(string) -> string", "Inverte a string"]],
        [36 * mm, 44 * mm, 90 * mm])

    sec("std.char -- Operacoes em Caracteres")
    p("As funcoes de <font name='Consolas'>std.char</font> operam sobre "
      "codigos Unicode (inteiros). Para usar com caracteres de uma string, "
      "obtenha o codigo via <font name='Consolas'>std.string.char_at()</font> "
      "primeiro.")

    code("""import std.char;
    is_alpha(c: int) -> bool         // e letra?
    is_digit_char(c: int) -> bool    // e digito?
    is_whitespace_char(c: int) -> bool // e whitespace?
    is_upper_char(c: int) -> bool    // e maiuscula?
    is_lower_char(c: int) -> bool    // e minuscula?
    is_alphanumeric(c: int) -> bool  // e alfanumerico?
    to_upper_char(c: int) -> int     // converte para maiuscula
    to_lower_char(c: int) -> int     // converte para minuscula""",
         "Exemplo: std.char.is_alpha(65) -> true ('A'); std.char.is_digit_char(48) -> true ('0').")

    sec("std.math -- Funcoes Matematicas")
    tbl(["Categoria", "Funcoes"],
        [["Inteiros", "abs, min, max, clamp, sign, gcd, lcm"],
         ["Floats", "abs_f, sqrt_f, pow_f, floor_f, ceil_f, round_f"],
         ["Trigonometricas", "sin_f, cos_f, tan_f, atan2_f (radianos)"],
         ["Logaritmicas", "log_f (ln), log2_f, log10_f"],
         ["Especiais", "is_nan_f, is_infinite_f"],
         ["Constantes", "pi(), e_const()"]],
        [24 * mm, 148 * mm])

    code("""import std.math as m;
    let pi = m.pi();                    // ~3.14159
    let area = pi * m.pow_f(5.0, 2.0);  // pi * r^2
    let hip = m.sqrt_f(9.0 + 16.0);     // sqrt(25) = 5.0
    let c = m.clamp(150, 0, 100);       // 100
    let g = m.gcd(12, 8);               // 4""", "Argumentos trigonometricos em radianos.")

    sec("std.convert -- Conversao de Tipos")
    code("""import std.convert;

    // Para string
    int_to_string(42)       -> "42"
    float_to_string(3.14)   -> "3.14"
    bool_to_string(true)    -> "true"

    // De string
    string_to_int("123")    -> 123
    string_to_float("3.14") -> 3.14
    string_to_int_or("abc", -1) -> -1  // com valor padrao

    // Numericas
    int_to_float(7)         -> 7.0
    float_to_int(9.9)       -> 9    (trunca, nao arredonda!)
    bool_to_int(true)       -> 1
    string_to_bool("true")  -> true (case-insensitive)""")

    # ================================================================
    chap(18, "std.collections, std.random, std.fs, std.env, std.time")

    sec("std.collections -- Listas Dinamicas")
    p("Listas sao gerenciadas via handles opacos (inteiros). Nao manipule "
      "handles diretamente -- use as funcoes do modulo.")

    code("""import std.collections as col;
    let lista = col.list_new();          // cria lista vazia

    // Mutacao
    col.list_push(lista, 10);            // adiciona ao final
    col.list_push(lista, 20);
    col.list_insert_at(lista, 1, 15);    // insere na posicao 1

    // Acesso
    let n = col.list_len(lista);         // 3
    let v = col.list_get(lista, 0);      // 10
    col.list_set(lista, 0, 99);          // modifica indice 0

    // Busca / ordenacao
    let tem = col.list_contains(lista, 20);   // bool
    let idx = col.list_index_of(lista, 20);   // indice ou -1
    col.list_sort(lista);                      // ordena in-place

    // Remocao
    let ultimo = col.list_pop(lista);          // remove e retorna ultimo
    let primeiro = col.list_pop_front(lista);  // remove e retorna primeiro

    // Lifecycle
    col.list_clear(lista);               // esvazia
    col.list_free(lista);                // libera memoria
    col.list_free_all();                 // libera todas""")

    sec("std.random, std.fs, std.env")
    code("""import std.random;
    random_seed(42);                     // semente para reproducao
    let dado = random_int(1, 6);         // 1..6 inclusive
    let f = random_float();              // [0.0, 1.0)
    let b = random_bool();               // true ou false""")

    code("""import std.fs;
    let conteudo = fs_read("dados.txt");       // le arquivo -> string
    let ok = fs_write("saida.txt", "Hello!");  // escreve (cria dirs pais)
    fs_append("log.txt", "nova linha\\n");     // adiciona ao final
    if fs_exists("config.txt") { /* ... */ }    // verifica existencia
    fs_remove("temp.txt");                      // remove arquivo""",
         "Retornos: fs_read retorna '' em erro; fs_write/fs_append retornam bool.")

    code("""import std.env;
    let home = env_get("HOME");          // obtem variavel de ambiente
    env_set("MINHA_VAR", "valor");       // define variavel
    let argc = env_args_count();         // numero de argumentos CLI
    for i in 0..argc {
        println(f"arg[{i}] = {env_arg(i)}");
    }""")

    sec("std.time -- Tempo e Duracao")
    code("""import std.time;

    // Wall clock
    let s = time_now_secs();              // segundos desde epoch Unix
    let ms = time_now_millis();           // milissegundos desde epoch Unix

    // Monotonic clock (nao afetado por ajustes do sistema)
    let m = monotonic_millis();
    let ns = monotonic_nanos();

    // Duration
    let d = duration_ms(500);             // 500ms
    let ds = duration_secs(5);            // 5s
    let dms = duration_millis(d);         // extrai ms
    let d2 = duration_add(d, d);          // soma com checagem de overflow
    sleep(d);                             // sleep com Duration

    // Instant
    let inicio = instant_now();           // captura instante
    let decorrido = instant_elapsed_ms(inicio);
    let futuro = instant_add(inicio, duration_ms(100));
    if instant_has_elapsed(futuro) { /* ... */ }

    // UTC
    let dt = unix_to_utc(time_now_secs());
    let ano = utc_year(dt);
    let mes = utc_month(dt);

    // Benchmark simples
    let t0 = time_now_millis();
    // ... operacao ...
    let t1 = time_now_millis();
    println(f"Duracao: {t1 - t0}ms")""")

    # ================================================================
    chap(19, "Async/Await -- Concorrencia de Primeira Classe")

    p("SpectraLang trata programacao assincrona como um conceito nativo da "
      "linguagem, nao como uma biblioteca adicionada depois. A instrucao "
      "<font name='Consolas'>AsyncReady</font> "
      "e o tipo <font name='Consolas'>Task&lt;T&gt;</font> existem no nivel "
      "da representacao intermediaria SSA. O compilador rebaixa cada "
      "<font name='Consolas'>await</font> para um host call de espera "
      "(<font name='Consolas'>spectra.async.task.wait</font>) que estaciona a "
      "lane atual no reactor da plataforma ate a tarefa terminar -- sem spin "
      "de CPU, sem callbacks, promises ou futures visiveis ao programador. O "
      "codigo continua sendo escrito como se fosse codigo sequencial normal.")

    p("O runtime de SpectraLang usa <font name='Consolas'>mio</font> sobre "
      "uma fila de prioridade, mapeando para <b>epoll</b> (Linux), "
      "<b>IOCP</b> (Windows) e <b>kqueue</b> (macOS), selecionado em tempo "
      "de compilacao. Este reactor de plataforma gerencia o escalonamento "
      "de tasks assincronas, fornecendo a camada de execucao sobre a qual "
      "o async/await opera. A seguranca e reforcada em tempo de compilacao: "
      "o compilador emite codigos de erro para valores nao-Send que cruzam "
      "pontos de <font name='Consolas'>await</font>, prevenindo condicoes de "
      "corrida e violacoes de seguranca de memoria.")

    code("""async fn ready_value() -> int { return 41; }

    async fn add_one() -> int {
        let value = await ready_value();
        value + 1
    }

    async fn from_block() -> int {
        let task: Task<int> = async {
            let base = await ready_value();
            base + 1
        };
        await task
    }

    fn main() -> int {
        let task = add_one();       // cria task sem bloquear
        let block_task = from_block();
        0
    }""", "Fonte: tests/validation/121_async_await_lowering.spectra")

    sec("Concorrencia com std.concurrent")
    code("""import std.concurrent as cc;

    let handle = cc.task_spawn(worker_fn);    // spawn de task
    let result = cc.task_join(handle);        // espera e obtem resultado

    // Channels
    let ch = cc.channel_new();
    cc.channel_send(ch, 42);
    let val = cc.channel_recv(ch);
    cc.channel_close(ch);

    // Counter atomico
    let ctr = cc.counter_new();
    cc.counter_add(ctr, 1);
    let v = cc.counter_get(ctr);

    // Pipeline
    let total = cc.pipeline_sum(inputs);""")

    # ================================================================
    chap(20, "Tensors e Autodiff")

    p("Este e o diferencial mais forte de SpectraLang: <b>dtype, rank, "
      "dimensoes, layout de memoria e dispositivo</b> sao parte do tipo e "
      "verificados em tempo de compilacao. Diferente de Python, onde um "
      "erro de forma de tensor so aparece no meio do treinamento (epoca 3, "
      "batch 47), SpectraLang rejeita incompatibilidades de rank, dtype, "
      "forma, layout e dispositivo antes de qualquer codigo executar. "
      "Codigos de erro E1401-E1406 cobrem cada categoria de incompatibilidade.")

    sec("Tipos Tensor Estaticos")
    code("""// Rank 1 com dimensao explicita
    let v: Tensor<float, rank1, dim3, row_major, cpu> = [1.0, 2.0, 3.0];

    // Rank 1 com dimensao dinamica
    let any_len: Tensor<float, rank1, dynamic_dim, row_major, cpu> = v;

    // Rank 2 (matriz)
    let m: Tensor<float, rank2, dim2, dim2, row_major, cpu> = [[1.0, 2.0], [3.0, 4.0]];

    // Funcao com tipo tensor estatico
    fn vector_total(values: Tensor<float, rank1, dim4, row_major, cpu>) -> int {
        return tensor.sum_f(values) as int;
    }""",
         "Fonte: tests/validation/102_pattern_tensor_ai_composition_stress.spectra")

    sec("API de Criacao e Metadados")
    code("""import std.tensor as tensor;

    let a = tensor.arange(1, 5, 1);     // [1, 2, 3, 4]
    let b = tensor.full(4, 2);          // [2, 2, 2, 2]
    let c = tensor.add(a, b);           // [3, 4, 5, 6]

    // Metadados
    let n = tensor.len(a);              // 4
    let r = tensor.rank(a);             // 1
    let rows = tensor.rows(m);          // 2

    // Acesso indexado
    let val = tensor.get(a, 0);         // 1
    tensor.set(a, 0, 99);               // modifica elemento

    // Reshape
    let m = tensor.reshape(tensor.arange(1, 7, 1), 2, 3);   // 2x3 matrix""")

    sec("Autodiff: Blocos diff e backward")
    p("<font name='Consolas'>diff { }</font> e um construct de linguagem que "
      "rebaixa para <font name='Consolas'>tensor.backward</font>. O bloco "
      "deve produzir um tensor escalar de loss. Operacoes de metadados "
      "dentro de <font name='Consolas'>diff { }</font> falham com codigo E1406.")

    code("""tensor.set_grad_enabled(true);

    let initial: Tensor<float, rank1> = [3.0, 3.0, 3.0];
    let weights = tensor.requires_grad(initial, true);

    let loss: Tensor<float, rank0> = diff {
        tensor.sum_t(tensor.mul(weights, weights))
    };

    let grad: Tensor<float, rank1> = tensor.grad(weights);
    // grad ~ [6.0, 6.0, 6.0] (d/dx de x^2 = 2x, sobre sum)""",
         "Autodiff e recurso do compilador. PyTorch detecta erro de forma no epoco 3; SpectraLang antes de executar.")

    # ================================================================
    chap(21, "std.ml -- Machine Learning")

    p("<font name='Consolas'>std.ml</font> fornece a camada de alto nivel "
      "para treinamento em CPU sobre handles de "
      "<font name='Consolas'>std.tensor</font>. Inclui modulos, camadas, "
      "funcoes de perda, otimizadores, datasets, dataloaders, experimentos, "
      "treinamento distribuido, exportacao ONNX, e componentes de transformer "
      "como atencao e embeddings posicionais.")

    code("""import std.tensor as tensor;
    import std.ml as ml;

    fn train_step(x, target, w, b) -> int {
        let pred = ml.linear(x, w, b);        // camada densa diferenciavel
        let loss = ml.mse_loss(pred, target);  // MSE loss
        tensor.backward(loss);                 // autodiff reverso
        ml.sgd_step(w, 0.1);                  // atualiza parametro
        ml.sgd_step(b, 0.1);
        0
    }""",
         "Fonte: examples/ai/mlp_training_serving.spectra")

    sec("Funcionalidades de ML")
    tbl(["Categoria", "Funcoes"],
        [["Module", "module_new, module_add_parameter, module_parameter_count"],
         ["Layers", "linear, conv2d, dropout, max_pool2d"],
         ["Losses", "mse_loss, bce_loss, cross_entropy_loss, nll_loss"],
         ["Optimizers", "sgd_step, sgd_momentum_step, adam_step, adamw_step, exp_lr"],
         ["Datasets", "dataset_from_tensors, dataset_from_csv, dataset_from_jsonl, dataset_from_npy"],
         ["Dataloaders", "dataloader_new, dataloader_batch_count, dataloader_batch_features"],
         ["Dataframes", "dataframe_from_csv, dataframe_rows, dataframe_cols, dataframe_column"],
         ["Experiments", "experiment_start, experiment_finish, experiment_log_metric"],
         ["ONNX", "onnx_export, onnx_import_summary, onnx_validate, onnx_roundtrip"],
         ["Transformers", "embedding_lookup, positional_encoding, attention, kv_cache_*, gelu, swiglu"],
         ["Tokenizer", "tokenizer_wordpiece, tokenizer_encode, tokenizer_decode, text_embed"],
         ["RAG", "rag_chunk_text, rag_build_prompt, rag_evaluate_answer, vector_index_*"],
         ["Evaluation", "metrics_classification, metrics_regression, serving_metrics, evaluation_report"]],
        [24 * mm, 150 * mm])

    # ================================================================
    # PARTE VIII: API PLATFORM (Cap 22)
    # ================================================================

    chap(22, "spectra.api -- HTTP, Routing, Middleware, e Database")

    p("O pacote <font name='Consolas'>spectra.api</font> e a plataforma nativa "
      "de API de SpectraLang. Tipos HTTP sao structs de primeira classe "
      "(<font name='Consolas'>Request</font>, <font name='Consolas'>Response</font>, "
      "<font name='Consolas'>Method</font>, <font name='Consolas'>Status</font>, "
      "<font name='Consolas'>Header</font>, <font name='Consolas'>Cookie</font>), "
      "nao mapas ou dicionarios genericos. O contrato define 211 host calls "
      "que cobrem HTTP/1.1 completo, JSON com derive macros, WebSocket, SSE, "
      "CORS, autenticacao, rate limiting, upload de arquivos, e drivers para "
      "SQLite, PostgreSQL e Redis. O servidor minimo cabe em 12 linhas:")

    sec("Hello HTTP -- Servidor Minimo")
    code("""module api.hello;

    import std.api.http;
    import std.api.routing;
    import std.api.handler;
    import std.api.server;
    import std.concurrent as cc;

    fn hello_handler(request: Request) -> Response {
        Response::ok("Hello, Spectra API!", "text/plain")
    }

    pub fn main() -> int {
        let router = Router::new();
        Router::get(router, "/hello", hello_handler);

        let srv = Server::new("127.0.0.1:8080");
        Server::serve(srv, router, |err: string| {
            println(f"Server error: {err}");
        });

        println("Server running on http://127.0.0.1:8080");
        return 0;
    }""",
         "Fonte: examples/api/00_hello_http.spectra")

    sec("Routing, Middleware, e JSON")
    code("""// Padroes de rota
    Router::get(router, "/users", list_users);           // literal
    Router::get(router, "/users/{id}", get_user);         // parametro
    Router::get(router, "/users/{id:\\\\d+}", get_user_id); // com regex
    Router::get(router, "/files/{*path}", serve_file);    // wildcard

    // Middleware chain
    fn logging_middleware(req: Request, next: fn(Request) -> Response) -> Response {
        println(f"{Method::to_string(req.method)} {req.path}");
        let resp = next(req);
        println(f"-> {Status::code(resp.status)}");
        resp
    }

    let chain = MiddlewareChain::new();
    MiddlewareChain::add(chain, logging_middleware);
    MiddlewareChain::add(chain, auth_middleware);
    Server::serve_with_middleware(srv, router, chain, error_handler);

    // JSON com derive macros
    #[derive(Serialize, Deserialize)]
    struct User {
        id: int,
        name: string,
        #[rename("email_address")]
        email: string,
        age: Option<int>
    }""")

    sec("Database Drivers")
    code("""// SQLite
    import std.api.sqlite as db;
    let conn = SqliteConnection::open("app.db");
    let stmt = SqliteStatement::prepare(conn,
        "CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)");
    SqliteStatement::step(stmt);
    SqliteStatement::finalize(stmt);

    // INSERT com bind parameters
    let insert = SqliteStatement::prepare(conn,
        "INSERT INTO users (name, email) VALUES (?, ?)");
    SqliteStatement::bind_text(insert, 1, "Alice");
    SqliteStatement::step(insert);

    // SELECT
    let query = SqliteStatement::prepare(conn,
        "SELECT id, name FROM users ORDER BY id");
    while SqliteStatement::step(query) != 0 {
        let id = SqliteStatement::column_int(query, 0);
        let name = SqliteStatement::column_text(query, 1);
        println(f"#{id}: {name}");
    }
    SqliteConnection::close(conn);""")

    p("Drivers adicionais via <font name='Consolas'>std.api.postgres</font> "
      "(prepared statements, transacoes, COPY, LISTEN/NOTIFY) e "
      "<font name='Consolas'>std.api.redis</font> (get, set, delete, expire, "
      "pub/sub). Observabilidade integrada: health checks "
      "(/startupz, /readyz, /healthz), Prometheus metrics, e OpenTelemetry "
      "tracing via OTLP/HTTP.")

    # Part 6: Chapters 23-30 -- Compiler, Toolchain, Appendices

    # ================================================================
    # PARTE IX: O COMPILADOR (Caps 23-25)
    # ================================================================

    chap(23, "O Pipeline de Compilacao")

    p("De um arquivo <font name='Consolas'>.spectra</font> a execucao, o "
      "caminho passa por tres grandes estagios mais a camada de runtime. "
      "O binario <font name='Consolas'>spectralang</font> orquestra tudo via "
      "o trait <font name='Consolas'>BackendDriver</font>, que isola o "
      "front-end do back-end -- permitindo que o LSP reutilize o front-end "
      "sem carregar Cranelift.")

    diag = flow_diagram([
        {"label": ".spectra", "sub": "fonte", "color": DARK},
        {"label": "Front-end", "sub": "lex/parse/sem/lint", "color": PRIMARY},
        {"label": "Mid-end", "sub": "IR SSA / TensorGraph", "color": SECONDARY},
        {"label": "Back-end", "sub": "Cranelift", "color": ACCENT},
        {"label": "Runtime", "sub": "init / main", "color": GREEN},
    ], cols=5, box_w=88, box_h=50, h_gap=12)
    story.append(diag)
    story.append(Paragraph(
        "Fluxo: cli/compiler_integration.rs -> CompilationPipeline::compile() (front-end) -> "
        "FullPipelineBackend::run()/execute() (mid+back-end) -> runtime.", style_caption))

    sec("As Quatro Crates Centrais")
    p("O compilador de SpectraLang e organizado em quatro crates Rust. "
      "O <b>spectra-compiler</b> contem o front-end completo: lexer, parser, "
      "definicao da AST, analise semantica multipassada e lint. Zero "
      "dependencias de Cranelift ou do runtime -- o que permite que o LSP, "
      "o formatador e o linter o reutilizem diretamente. O "
      "<b>spectra-midend</b> implementa o rebaixamento da AST para IR SSA "
      "(SIR), o TensorGraph (uma segunda IR de mais alto nivel para operacoes "
      "tensoriais), passes de otimizacao como constant folding e dead code "
      "elimination, e o no nativo de autodiff. O <b>spectra-backend</b> "
      "contem o codegen Cranelift tanto para JIT quanto para AOT. E o "
      "<b>spectra-runtime</b> fornece os servicos de execucao: inicializacao, "
      "gerenciamento hibrido de memoria, registro de host calls, e o reactor "
      "de eventos para async/await.")

    code("""main() -> run_cli() -> parse_cli() -> execute_build_command()
      -> execute_plan_with_sources() -> ProjectPlan::build_with_sources()
      -> topological_order() -> SPECTRACompiler::compile() (por modulo)
      -> CompilationPipeline::compile() + FullPipelineBackend::run/execute()
      -> spectra_runtime::initialize();
         codegen.execute_entry_point("main", ir)""",
         "Fonte: tools/spectra-cli/src/{main,compiler_integration}.rs")

    # ================================================================
    chap(24, "Front-end: Lexer, Parser e Semantica")

    sec("Lexer")
    p("O lexer e um automato finito escrito a mao sobre "
      "<font name='Consolas'>char_indices</font> da string fonte. Ele emite "
      "tokens com spans precisos (offset de byte + linha/coluna), suportando "
      "comentarios de linha, f-strings, literais de caractere, sequencias de "
      "escape e operadores compostos (<font name='Consolas'>==</font>, "
      "<font name='Consolas'>&lt;=</font>, <font name='Consolas'>&&</font>, "
      "<font name='Consolas'>-></font>, <font name='Consolas'>..=</font>). "
      "Erros lexicos recebem codigos L001-L099 com spans apontando exatamente "
      "para o caractere problematico.")

    sec("Parser")
    p("O parser e de descida recursiva com 1 token de lookahead e "
      "<b>recuperacao de erro</b> via <font name='Consolas'>synchronize()</font>, "
      "que reporta multiplos erros por passada em vez de abortar no primeiro. "
      "O <font name='Consolas'>ModuleLoader</font> implementa cache "
      "incremental: hash de <i>source + feature flags</i> evita re-lexar "
      "e re-parser em cache hit. Erros de parser recebem codigos P001-P999.")

    sec("Analise Semantica Multipassada")
    p("A analise semantica usa um padrao visitor sobre a AST em multiplas "
      "passadas. A passada 0 resolve importacoes via "
      "<font name='Consolas'>ModuleRegistry</font> compartilhado. A passada 1 "
      "coleta declaracoes e detecta duplicatas. A passada 2 analisa corpos "
      "de funcoes, gerencia a pilha de escopos, verifica retornos e valida "
      "implementacoes de trait. A passada 3 infere argumentos de tipo "
      "generico. A passada 4 preenche <font name='Consolas'>type_name</font> "
      "em MethodCall para o mid-end. Erros semanticos usam codigos E001-E099, "
      "com codigos especializados E1401-E1406 para tensores e E2101-E2120 "
      "para async. Diagnosticos sao exportaveis em JSON estavel ou SARIF 2.1.0.")

    # ================================================================
    chap(25, "Mid-end, Back-end e Runtime")

    sec("IR SSA (SIR)")
    p("A representacao intermediaria e baseada em forma SSA: cada funcao e "
      "um grafo de <font name='Consolas'>BasicBlock</font>s com "
      "<font name='Consolas'>Instruction</font>s e um "
      "<font name='Consolas'>Terminator</font>. Valores sao identificados por "
      "<font name='Consolas'>Value { id }</font>, unicos por funcao. "
      "Terminadores incluem <font name='Consolas'>Return, Branch, CondBranch, "
      "Switch, Unreachable</font>. Instrucoes cobrem aritmetica, memoria, "
      "chamadas, PHI, constantes tipadas, Cast, e operacoes especificas de "
      "linguagem como <font name='Consolas'>AutodiffStep</font> (no nativo de "
      "autodiff reverso) e <font name='Consolas'>AsyncReady</font>. Genericos "
      "sao monomorfizados com name mangling e teto de 512 especializacoes.")

    sec("Otimizacao")
    p("O mid-end implementa passes de otimizacao via o trait "
      "<font name='Consolas'>Pass { fn name() -> &str; fn run(&mut self, "
      "module: &mut Module) -> bool; }</font>. "
      "<font name='Consolas'>ConstantFolding</font> ativo em opt-level >= 1. "
      "<font name='Consolas'>FunctionInlining + DeadCodeElimination</font> em "
      ">= 2. <font name='Consolas'>ConcurrentSpawnJoinFusion</font> quando "
      "optimize ativo. <font name='Consolas'>LoopStructureValidation</font> "
      "roda sempre. <font name='Consolas'>verify_module()</font> roda antes e "
      "depois de cada otimizacao.")

    sec("Back-end Cranelift e Runtime")
    p("O back-end JIT usa <font name='Consolas'>JITBuilder</font> com "
      "optimizer 'speed'. Cada basic block da IR vira um bloco Cranelift; "
      "instrucoes PHI viram block parameters (Cranelift nao tem PHI nativo). "
      "O AOT via <font name='Consolas'>cranelift_object</font> emite "
      "COFF/ELF/Mach-O. O runtime implementa <font name='Consolas'>HybridMemory</font>: "
      "GC de rastreamento (<font name='Consolas'>Gc&lt;T&gt;</font>) para "
      "valores gerenciados e alocacao manual para scratch de baixo nivel. "
      "A ABI de host-call usa <font name='Consolas'>SpectraHostValue</font> (int64_t) "
      "com contexto de argumentos e resultados. O contrato spectra.api define "
      "exatamente 211 host calls assereadas em tempo de compilacao.")

    # ================================================================
    # PARTE X: TOOLCHAIN (Caps 26-27)
    # ================================================================

    chap(26, "Toolchain: CLI, Package Manager, LSP")

    sec("O Binario spectralang")
    p("O binario <font name='Consolas'>spectralang</font> centraliza todos "
      "os comandos da toolchain. <font name='Consolas'>compile</font> compila "
      "modulos, <font name='Consolas'>check</font> verifica tipos sem gerar "
      "codigo, <font name='Consolas'>run</font> compila e executa via JIT, "
      "<font name='Consolas'>lint</font> executa regras de lint, "
      "<font name='Consolas'>fmt</font> formata codigo-fonte, "
      "<font name='Consolas'>repl</font> inicia o REPL interativo, e "
      "<font name='Consolas'>new</font> cria um novo projeto. Niveis de "
      "otimizacao: -O0 (sem otimizacoes), -O1 (basicas), -O2 (moderadas, "
      "padrao), -O3 (agressivas). Diagnosticos exportaveis em JSON ou SARIF.")

    sec("Package Manager")
    p("<font name='Consolas'>spectralang package</font> oferece 19 subcomandos "
      "(lock, build, test, add, publish, catalog, etc.). O arquivo de lock "
      "<font name='Consolas'>spectrum.lock</font> e deterministico com SHA-256. "
      "O manifesto <font name='Consolas'>spectra.toml</font> contem secoes "
      "[project], [release] e [dependencies]. Suporte a workspaces multi-crate "
      "e resolucao de dependencias transitivas.")

    sec("LSP e Ferramentas de Desenvolvimento")
    p("<font name='Consolas'>spectra-lsp</font> implementa 14 capacidades: "
      "hover, go-to-definition, rename, completion, inlay hints, semantic "
      "tokens, quickfix diagnostics, document symbols, formatting, code "
      "actions, folding ranges, signature help, references, e workspace "
      "symbols. O LSP reutiliza o front-end do compilador diretamente, sem "
      "carregar Cranelift ou o runtime. O formatador usa indentacao de 4 "
      "espacos e max 100 caracteres por linha. O linter tem 3 regras: "
      "unused-binding, unreachable-code e shadowing. "
      "<font name='Consolas'>spectralang test</font> executa testes do projeto.")

    code("""$ spectralang repl
    spectra> let x = 42
    spectra> x * 2
    84
    spectra> f"Ola, {x}!"
    "Ola, 42!"
    spectra> :quit""")

    # ================================================================
    chap(27, "Performance, Maturidade e Roadmap")

    p("SpectraLang e uma linguagem em desenvolvimento ativo, com foco "
      "disciplinado em qualidade de engenharia. O roadmap e rastreado em "
      "tres artefatos: o plano estrategico de implementacao, o backlog "
      "humano, e o roadmap estruturado em TOML. Aproximadamente 67% dos 267 "
      "itens do roadmap estao concluidos, cobrindo as Fases 0-22 (nucleo de "
      "IA/ML, async core, API foundation).")

    sec("Benchmarks Cross-Linguagem")
    p("31 cenarios x Spectra/Go/Java/Rust, 3 warmups, 20 amostras, gate de "
      "drift &lt;=15%:")
    tbl(["Cenario", "vs Go", "vs Rust"],
        [["tensor-reduce", "1.05x (paridade)", "1.6x"],
         ["tensor-elementwise", "1.2x", "2.0x"],
         ["tensor-matmul", "1.94x", "2.5x"],
         ["cpu-hashmap", "6.0x", "6.9x"],
         ["cpu-string-build", "71.7x (gap conhecido)", "66.9x"]],
        [60 * mm, 50 * mm, 50 * mm])

    sec("Maturidade das Features da Linguagem")
    tbl(["Feature", "Status"],
        [["Variables, primitives, operators", "Stable"],
         ["Functions, implicit return, generics", "Stable"],
         ["Control flow (if/unless/while/for/loop/do-while)", "Stable"],
         ["switch, match, if let, while let", "Stable"],
         ["Arrays, tuples, ranges", "Stable"],
         ["Structs, enums, impl methods", "Stable"],
         ["Traits, generic bounds, closures", "Stable"],
         ["Module system, visibility (pub/internal)", "Stable"],
         ["Option, Result, ? operator", "Stable"],
         ["F-strings, implicit block return", "Stable"],
         ["Tensor types (static rank/dtype/dims)", "Stable"],
         ["Autodiff (diff blocks, backward, grad)", "Stable"],
         ["Async/await, Task<T>", "Stable"],
         ["spectra.api (HTTP, routing, middleware, JSON)", "Stable"],
         ["SQLite/Postgres/Redis drivers", "Stable"],
         ["Otel, Prometheus, health checks", "Stable"],
         ["Package manager, LSP, formatter", "Stable"],
         ["Exact-width numerics (i8..i64, f32, f64)", "In progress"],
         ["Dynamic overflow diagnostics", "In progress"],
         ["GPU backends (CUDA, ROCm, Metal, Vulkan)", "Future/Reserved"],
         ["Distributed training (real multi-node)", "Future"]],
        [80 * mm, 90 * mm])

    nota("Status honesto: ainda nao e v1.0 estavel. JIT e o caminho primario; "
         "exe standalone nao esta totalmente integrado; a stdlib tem gaps relativos "
         "ao plano. Posicionamento: base de engenharia solida e disciplinada, "
         "nao promessa exagerada.")

    # ================================================================
    # PARTE XI: APENDICES (Caps 28-30)
    # ================================================================

    chap(28, "Apendice A: Referencia Rapida")

    sec("Palavras-chave (39)")
    code("""module import pub internal fn struct enum impl trait
    let mut self Self if elif elseif else unless while
    do for in of loop match switch case return break
    continue true false async await""", skinny_nums=True)

    sec("Precedencia de Operadores")
    tbl(["Nivel", "Operadores", "Associatividade"],
        [["1", "() f() x.y x[i]", "Esq."],
         ["2", "- ! (unarios)", "Dir."],
         ["3", "* / %", "Esq."],
         ["4", "+ -", "Esq."],
         ["5", "< > <= >=", "Esq."],
         ["6", "== !=", "Esq."],
         ["7", "&&", "Esq."],
         ["8", "||", "Esq."]],
        [12 * mm, 60 * mm, 24 * mm])

    sec("Visibilidade")
    tbl(["Modificador", "Acessivel de"],
        [["(padrao)", "Apenas no modulo atual"],
         ["pub", "Qualquer codigo"],
         ["internal", "Mesmo pacote/projeto"]],
        [30 * mm, 132 * mm])

    sec("Formas de import")
    code("""import std.io;                        // qualificada
    import std.math as math;               // alias
    import { println } from std.io;        // por nome
    pub import { println } from std.io;    // re-export""", skinny_nums=True)

    sec("Erros Comuns")
    tbl(["Erro", "Causa", "Solucao"],
        [["type mismatch", "Operacao mista int+float", "Use int_to_float() ou float_to_int()"],
         ["non-exhaustive match", "Casos faltando no match", "Adicione _ => ou os casos faltantes"],
         ["break outside loop", "break fora de loop", "Mova para dentro de while/for/loop"],
         ["module declaration missing", "Arquivo sem 'module nome;'", "Adicione 'module nome;' no topo"],
         ["main not found", "Sem ponto de entrada", "Adicione 'pub fn main() -> int { return 0; }'"],
         ["undefined variable", "Uso fora do escopo", "Garanta que 'let' esta no escopo correto"],
         ["field not found", "Campo inexistente", "Verifique o nome do campo na struct"]],
        [36 * mm, 50 * mm, 86 * mm])

    # ================================================================
    chap(29, "Apendice B: Gramatica e Mapa de Arquivos")

    sec("Gramatica Informal (EBNF)")
    code("""programa      = declaracao_modulo? import* declaracao* ;
    declaracao_modulo = "module" IDENT ";" ;
    import        = "import" caminho_modulo ";"
                  | "import" caminho_modulo "as" IDENT ";"
                  | "import" "{" IDENT ("," IDENT)* "}" "from" caminho_modulo ";"
                  | "pub" "import" "{" IDENT ("," IDENT)* "}" "from" caminho_modulo ";";
    declaracao    = decl_fn | decl_struct | decl_enum | decl_impl | decl_trait ;
    decl_fn       = visib? "fn" IDENT genericos? "(" params? ")" ("->" tipo)? bloco ;
    decl_struct   = visib? "struct" IDENT genericos? "{" campo* "}" ;
    decl_enum     = visib? "enum" IDENT genericos? "{" variante* "}" ;
    variante      = IDENT                                   (* unit *)
                  | IDENT "(" tipo ("," tipo)* ")"          (* tupla *)
                  | IDENT "{" campo* "}"                    (* struct *);
    decl_impl     = "impl" genericos? IDENT "{" metodo* "}" ;
    decl_trait    = visib? "trait" IDENT (";" IDENT)? "{" assinatura* "}" ;
    bloco         = "{" stmt* expr? "}" ;
    stmt          = decl_let | atribuicao | retorno | expr ";"
                  | laco | condicional | match_stmt ;
    decl_let      = "let" "mut"? IDENT (":" tipo)? "=" expr ";" ;
    expr          = literal | IDENT | binario | unario | chamada
                  | met_call | f_string | array_lit | tuple_lit
                  | "if" expr bloco ("elif" expr bloco)* ("else" bloco)?
                  | "unless" expr bloco ("else" bloco)?
                  | "match" expr "{" braco* "}" | closure ;
    tipo          = "int" | "float" | "bool" | "string" | "char"
                  | IDENT | IDENT "<" tipo ("," tipo)* ">"
                  | "[" tipo "]" | "(" tipo ("," tipo)* ")" ;
    visib         = "pub" | "internal" ;""", skinny_nums=True)

    sec("Mapa de Arquivos do Repositorio")
    code("""compiler/src/lexer/        Lexer (tokens, spans)
    compiler/src/parser/       Parser (recursivo), ModuleLoader (cache)
    compiler/src/ast/          Arvore sintatica
    compiler/src/semantic/     Analise semantica multipassada
    compiler/src/lint/         Regras de lint
    compiler/src/pipeline.rs   CompilationPipeline + trait BackendDriver
    midend/src/ir.rs           IR SSA (SIR) -- instrucoes, blocos, tipos
    midend/src/lowering.rs     AST -> IR (rebaixamento)
    midend/src/autodiff.rs     AutodiffStep (no nativo de autodiff)
    midend/src/tensor_graph.rs TensorGraph (segunda IR)
    midend/src/passes/         Passes de otimizacao/verificacao
    backend/src/codegen.rs     Cranelift JIT
    backend/src/aot.rs         Cranelift AOT (COFF/ELF/Mach-O)
    runtime/src/memory/        HybridMemory (GC + manual)
    runtime/src/ffi.rs         Host-call ABI (SpectraHostCallContext)
    runtime/src/reactor/       Reactor async (epoll/IOCP/kqueue)
    runtime/src/api/           Contrato spectra.api (211 calls)
    runtime/src/stdlib/        Registro de host functions da stdlib
    tools/spectra-cli/         Binario 'spectralang'
    tools/spectra-lsp/         LSP (14 capacidades)
    tools/spectra-interop/     Interop (Python, etc.)
    packages/spectra-api/      Implementacao spectra.api
    tests/validation/          172 arquivos .spectra (regressao)
    tests/errors/              73 arquivos .spectra (negativos)
    examples/ai/               21 exemplos de IA/ML
    examples/api/              5 exemplos de API""", skinny_nums=True)

    # ================================================================
    chap(30, "Apendice C: Stdlib Completa e Roadmap")

    sec("Todos os Modulos stdlib")
    tbl(["Modulo", "Funcoes principais"],
        [["std.io", "print, println, eprint, eprintln, read_line, input, flush"],
         ["std.string", "len, contains, to_upper, to_lower, trim, starts_with, ends_with, concat, repeat_str, char_at, substring, replace, index_of, split_first, split_last, count_occurrences, is_empty, pad_left, pad_right, reverse_str"],
         ["std.math", "abs, min, max, clamp, sign, gcd, lcm, abs_f, sqrt_f, pow_f, floor_f, ceil_f, round_f, sin_f, cos_f, tan_f, atan2_f, log_f, log2_f, log10_f, is_nan_f, is_infinite_f, pi, e_const"],
         ["std.convert", "int_to_string, float_to_string, bool_to_string, string_to_int, string_to_float, string_to_int_or, string_to_bool, int_to_float, float_to_int, bool_to_int"],
         ["std.collections", "list_new, list_push, list_pop, list_pop_front, list_len, list_get, list_set, list_insert_at, list_remove_at, list_contains, list_index_of, list_sort, list_map, list_filter, list_reduce, list_clear, list_free, list_free_all"],
         ["std.random", "random_seed, random_int, random_float, random_bool"],
         ["std.fs", "fs_read, fs_write, fs_append, fs_exists, fs_remove"],
         ["std.env", "env_get, env_set, env_args_count, env_arg"],
         ["std.option", "is_some, is_none, option_unwrap, option_unwrap_or"],
         ["std.result", "is_ok, is_err, result_unwrap, result_unwrap_or, result_unwrap_err"],
         ["std.char", "is_alpha, is_digit_char, is_whitespace_char, is_upper_char, is_lower_char, is_alphanumeric, to_upper_char, to_lower_char"],
         ["std.time", "time_now_millis, time_now_secs, monotonic_millis, monotonic_nanos, sleep_ms, sleep, duration_ms, duration_secs, duration_millis, duration_add, duration_sub, instant_now, instant_elapsed_ms, instant_add, instant_has_elapsed, unix_to_utc, utc_year/month/day/hour/minute/second"],
         ["std.range", "create, len, at, eq, start, end, is_inclusive"],
         ["std.tensor", "Ver Capitulos 5 e 20 para referencia completa (50+ funcoes)"],
         ["std.ml", "Ver Capitulo 21 para referencia completa (50+ funcoes)"],
         ["std.concurrent", "task_spawn, task_join, channel_new, channel_send, channel_recv, channel_close, counter_new, counter_add, counter_get, pipeline_sum"],
         ["std.serve", "server_new, server_warmup, server_enqueue, server_process_batch, server_result, server_set_input_policy, server_set_output_policy, server_set_rate_limit, server_set_fallback, server_set_model_version, server_monitoring_snapshot, server_distribution_summary, drift_check, export_monitoring"]],
        [24 * mm, 150 * mm])

    sec("Flags do CLI -- Resumo Completo")
    tbl(["Flag", "Descricao"],
        [["compile", "Compila modulos (padrao)"],
         ["check", "Verifica tipos sem gerar codigo"],
         ["run", "Compila e executa via JIT"],
         ["lint", "Executa verificacoes de lint"],
         ["fmt", "Formata arquivos fonte"],
         ["repl", "Inicia o REPL interativo"],
         ["new", "Cria um novo projeto"],
         ["-O0 / --no-optimize", "Desativa otimizacoes"],
         ["-O1", "Otimizacoes basicas"],
         ["-O2", "Otimizacoes moderadas (padrao)"],
         ["-O3", "Todas as otimizacoes"],
         ["--dump-ast", "Exibe a AST para debug"],
         ["--dump-ir", "Exibe o IR para debug"],
         ["--timings / -T", "Metricas de compilacao"],
         ["--summary", "Sumario do pipeline por modulo"],
         ["--verbose / -v", "Detalhes adicionais do build"],
         ["--emit-object <out>", "Gera arquivo objeto AOT"],
         ["--emit-exe <out>", "Gera executavel AOT"],
         ["--json", "Diagnosticos em JSON"],
         ["--sarif", "Diagnosticos SARIF 2.1.0 (GitHub Code Scanning)"],
         ["--lint", "Ativa verificacoes de lint"],
         ["--allow <rule>", "Suprime uma regra de lint"],
         ["--deny <rule>", "Eleva regra de lint a erro"]],
        [56 * mm, 118 * mm])

    sec("Fases do Roadmap")
    tbl(["Fase", "Descricao", "Status"],
        [["0-2", "Nucleo da linguagem (lexer, parser, AST, semantica)", "Complete"],
         ["3-7", "Tensors, kernels, autodiff, ML framework, dispositivos", "Complete"],
         ["8-13", "Interop, package manager, tooling, testes", "Complete"],
         ["14-20", "IR, tensor graph, GPU, datasets, ONNX, RAG, avaliacao", "Complete"],
         ["21", "Async/await core e reactor", "Complete"],
         ["22", "API foundation (HTTP/1.1, routing, middleware)", "Complete"],
         ["23", "Middleware e seguranca avancada", "In progress"],
         ["24", "Recursos avancados de API (HTTP/2, HTTP/3, SSE, GraphQL)", "In progress"],
         ["25", "Persistencia e database drivers", "In progress"],
         ["26", "API tooling e DX", "Not started"],
         ["27", "Observabilidade e operacoes", "In progress"],
         ["28", "API conformance v1.0 e release", "Not started"],
         ["29-31", "Exact-width numerics, string/buffer optimizations", "In progress"]],
        [16 * mm, 76 * mm, 28 * mm])

    sec("Documentos de Apoio no Repositorio")
    p("docs/reference/01-06: Referencia completa da linguagem (6 partes). "
      "docs/book/01-10: The Spectra Book. docs/AI-AGENT-REFERENCE.md: "
      "Referencia para agentes AI. docs/language-feature-maturity.md: "
      "Matriz de maturidade. docs/api/*: 24 docs de referencia da API. "
      "docs/adr/*: Architecture Decision Records (0001-0011). "
      "docs/diagnostics/error-code-reference.md: Referencia de codigos de erro. "
      "docs/ARCHITECTURE.md: Arquitetura geral. "
      "docs/production-ai-implementation-plan.md: Plano estrategico. "
      "roadmap/roadmap.toml: Roadmap estruturado (machine-readable). "
      "docs/roadmap-backlog.md: Backlog humano.")

    sec("Fim do Guia")
    p("Este guia cobre a linguagem SpectraLang versao 0.3.0 de forma "
      "abrangente, desde os fundamentos absolutos da programacao ate os "
      "recursos mais avancados da linguagem e sua toolchain. Para "
      "contribuicoes, bugs ou duvidas, consulte o repositorio do projeto. "
      "O codigo-fonte deste ebook esta em "
      "<font name='Consolas'>presentation/spectra_ebook_pdf.py</font>. "
      "Para regenera-lo, execute "
      "<font name='Consolas'>python presentation/spectra_ebook_pdf.py</font>.")

    code("""python presentation/spectra_ebook_pdf.py
    # Gera: presentation/spectra_ebook.pdf""", skinny_nums=True)



# --------------------------------------------------------------------------- #
# Main
# --------------------------------------------------------------------------- #
def main():
    here = os.path.dirname(os.path.abspath(__file__))
    out = os.path.join(here, "spectra_ebook.pdf")
    doc = EbookDoc(
        out, pagesize=A4,
        leftMargin=LMARGIN, rightMargin=RMARGIN,
        topMargin=TMARGIN, bottomMargin=BMARGIN,
        title="SpectraLang -- Guia Completo da Linguagem",
        author="SpectraLang")
    frame = Frame(LMARGIN, BMARGIN, USABLE, PAGE_H - TMARGIN - BMARGIN, id="main")
    doc.addPageTemplates([PageTemplate(id="all", frames=[frame],
                                       onPage=_header_footer)])
    build_content()
    doc.multiBuild(story)
    print("Ebook gerado:", out)


if __name__ == "__main__":
    main()
