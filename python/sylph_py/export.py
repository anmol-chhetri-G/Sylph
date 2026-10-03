"""
Sylph Export Module
Document export functions called from Rust via PyO3.

Supports: DOCX, PDF, Markdown, HTML
"""

import os
import re
from typing import List, Tuple

# Import the renderers' libraries once, when this module loads. Importing
# them lazily inside the functions let two exports on different threads
# import python-docx at the same time, and its circular imports then
# deadlock (_DeadlockError on docx.enum). The function-level imports below
# are now just lookups in sys.modules.
import docx  # noqa: E402,F401
import docx.enum.section  # noqa: F401
import docx.enum.text  # noqa: F401
import docx.opc.constants  # noqa: F401
import docx.oxml  # noqa: F401
import docx.oxml.ns  # noqa: F401
import docx.shared  # noqa: F401
import fpdf  # noqa: F401
import fpdf.fonts  # noqa: F401


def _parse_inline(text: str) -> List[Tuple[str, dict]]:
    """Parse inline markdown formatting into segments with styles.

    Returns list of (text, {'bold': bool, 'italic': bool, 'code': bool,
    'strike': bool, 'link': str|None})
    """
    segments = []
    i = 0
    n = len(text)

    while i < n:
        # Strikethrough: ~~text~~
        m = re.match(r'~~(.+?)~~', text[i:])
        if m:
            segments.append((m.group(1), {'bold': False, 'italic': False, 'code': False, 'strike': True}))
            i += m.end()
            continue

        # Bold + Italic: ***text*** or ___text___
        m = re.match(r'\*\*\*(.+?)\*\*\*', text[i:])
        if not m:
            m = re.match(r'___(.+?)___', text[i:])
        if m:
            segments.append((m.group(1), {'bold': True, 'italic': True, 'code': False}))
            i += m.end()
            continue

        # Bold: **text** or __text__
        m = re.match(r'\*\*(.+?)\*\*', text[i:])
        if not m:
            m = re.match(r'__(.+?)__', text[i:])
        if m:
            segments.append((m.group(1), {'bold': True, 'italic': False, 'code': False}))
            i += m.end()
            continue

        # Italic: *text* or _text_
        m = re.match(r'\*(.+?)\*', text[i:])
        if not m:
            m = re.match(r'(?<!\w)_(.+?)_(?!\w)', text[i:])
        if m:
            segments.append((m.group(1), {'bold': False, 'italic': True, 'code': False}))
            i += m.end()
            continue

        # Inline code: `text`
        m = re.match(r'`(.+?)`', text[i:])
        if m:
            segments.append((m.group(1), {'bold': False, 'italic': False, 'code': True}))
            i += m.end()
            continue

        # Link: [text](url)
        m = re.match(r'\[(.+?)\]\((.+?)\)', text[i:])
        if m:
            segments.append((m.group(1), {'bold': False, 'italic': False, 'code': False, 'link': m.group(2)}))
            i += m.end()
            continue

        # Plain text: consume until next special character
        j = i + 1
        while j < n and text[j] not in ('*', '_', '`', '[', '~'):
            j += 1
        segments.append((text[i:j], {'bold': False, 'italic': False, 'code': False}))
        i = j

    return segments if segments else [('', {'bold': False, 'italic': False, 'code': False})]


def _is_table_delimiter(line: str) -> bool:
    """True for `| --- | --- |` style delimiter rows."""
    cells = [c.strip() for c in line.strip().strip('|').split('|')]
    if not cells:
        return False
    return all(re.match(r'^:?-{1,}:?$', c) for c in cells if c != '')


def _split_table_row(line: str) -> List[str]:
    return [c.strip() for c in line.strip().strip('|').split('|')]


def _parse_markdown_lines(text: str) -> List[dict]:
    """Parse markdown text into structured blocks.

    Returns list of dicts with 'type' and content fields.
    Types: heading, paragraph, code_block, blockquote, hr, ul, ol, table, blank
    """
    blocks = []
    lines = text.split('\n')
    i = 0

    while i < len(lines):
        line = lines[i]

        # Blank line
        if line.strip() == '':
            blocks.append({'type': 'blank'})
            i += 1
            continue

        # Fenced code block
        if line.strip().startswith('```'):
            lang = line.strip()[3:].strip()
            code_lines = []
            i += 1
            while i < len(lines) and not lines[i].strip().startswith('```'):
                code_lines.append(lines[i])
                i += 1
            blocks.append({'type': 'code_block', 'lang': lang, 'text': '\n'.join(code_lines)})
            i += 1  # skip closing ```
            continue

        # Horizontal rule
        if re.match(r'^(\*\*\*+|---+|___+)\s*$', line.strip()):
            blocks.append({'type': 'hr'})
            i += 1
            continue

        # Heading
        m = re.match(r'^(#{1,6})\s+(.+)$', line)
        if m:
            level = len(m.group(1))
            blocks.append({'type': 'heading', 'level': level, 'text': m.group(2)})
            i += 1
            continue

        # Blockquote
        if line.startswith('>'):
            quote_lines = []
            while i < len(lines) and lines[i].startswith('>'):
                quote_lines.append(lines[i][1:].strip())
                i += 1
            blocks.append({'type': 'blockquote', 'text': '\n'.join(quote_lines)})
            continue

        # Unordered list
        m = re.match(r'^(\s*)[-*+]\s+(.+)$', line)
        if m:
            items = []
            while i < len(lines):
                lm = re.match(r'^(\s*)[-*+]\s+(.+)$', lines[i])
                if lm:
                    items.append(lm.group(2))
                    i += 1
                else:
                    break
            blocks.append({'type': 'ul', 'items': items})
            continue

        # Ordered list
        m = re.match(r'^(\s*)\d+\.\s+(.+)$', line)
        if m:
            items = []
            while i < len(lines):
                lm = re.match(r'^(\s*)\d+\.\s+(.+)$', lines[i])
                if lm:
                    items.append(lm.group(2))
                    i += 1
                else:
                    break
            blocks.append({'type': 'ol', 'items': items})
            continue

        # Table: header row + delimiter row + body rows
        if '|' in line and i + 1 < len(lines) and _is_table_delimiter(lines[i + 1]):
            header = _split_table_row(line)
            i += 2  # skip header + delimiter
            rows = [header]
            while i < len(lines) and '|' in lines[i] and lines[i].strip() != '':
                rows.append(_split_table_row(lines[i]))
                i += 1
            blocks.append({'type': 'table', 'rows': rows})
            continue

        # Paragraph (collect consecutive non-special lines)
        para_lines = []
        while i < len(lines) and lines[i].strip() != '':
            nxt = lines[i]
            if (nxt.strip().startswith('```') or
                nxt.strip().startswith('#') or
                nxt.strip().startswith('>') or
                re.match(r'^(\s*)[-*+]\s+', nxt) or
                re.match(r'^(\s*)\d+\.\s+', nxt) or
                re.match(r'^(\*\*\*+|---+|___+)\s*$', nxt.strip()) or
                ('|' in nxt and i + 1 < len(lines) and _is_table_delimiter(lines[i + 1]))):
                break
            para_lines.append(nxt)
            i += 1
        if para_lines:
            blocks.append({'type': 'paragraph', 'text': ' '.join(para_lines)})

    return blocks


def _apply_inline_docx(run, styles):
    """Apply parsed inline styles to a python-docx run."""
    from docx.shared import Pt
    # None inherits from the paragraph style (bold headings); False would
    # switch the style's bold off.
    run.bold = True if styles.get('bold') else None
    run.italic = True if styles.get('italic') else None
    if styles.get('strike', False):
        run.font.strike = True
    if styles.get('code', False):
        run.font.name = 'Courier New'
        run.font.size = Pt(10)


def _link_url(styles):
    """Extract the URL from run styles, or None when the run is not a link.

    Styles are a list of either bare strings ("Bold") or one-element dicts
    ({"Link": "https://..."}).
    """
    for s in styles:
        if isinstance(s, dict) and 'Link' in s:
            return s['Link']
    return None


def _add_hyperlink_docx(paragraph, text, url, *, bold=False, italic=False,
                        code=False, strike=False, size=None, font=None):
    """Append a real OOXML hyperlink run to a paragraph."""
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn
    from docx.opc.constants import RELATIONSHIP_TYPE as RT

    r_id = paragraph.part.relate_to(url, RT.HYPERLINK, is_external=True)
    hyperlink = OxmlElement('w:hyperlink')
    hyperlink.set(qn('r:id'), r_id)

    run_el = OxmlElement('w:r')
    r_pr = OxmlElement('w:rPr')
    # Children in the order the OOXML schema (CT_RPr) requires: Word
    # reports "unreadable content" for out-of-order run properties.
    if code or font:
        family = 'Courier New' if code else font
        fonts = OxmlElement('w:rFonts')
        fonts.set(qn('w:ascii'), family)
        fonts.set(qn('w:hAnsi'), family)
        r_pr.append(fonts)
    if bold:
        r_pr.append(OxmlElement('w:b'))
    if italic:
        r_pr.append(OxmlElement('w:i'))
    if strike:
        r_pr.append(OxmlElement('w:strike'))
    color = OxmlElement('w:color')
    color.set(qn('w:val'), '0563C1')
    r_pr.append(color)
    if code or size:
        size_el = OxmlElement('w:sz')
        # Half-points; code keeps its 10 pt.
        size_el.set(qn('w:val'), '20' if code else str(int(round(float(size) * 2))))
        r_pr.append(size_el)
    underline = OxmlElement('w:u')
    underline.set(qn('w:val'), 'single')
    r_pr.append(underline)
    run_el.append(r_pr)
    text_el = OxmlElement('w:t')
    text_el.set(qn('xml:space'), 'preserve')
    text_el.text = text
    run_el.append(text_el)
    hyperlink.append(run_el)
    paragraph._p.append(hyperlink)


def _add_styled_run_docx(paragraph, run_data):
    """Add one run (text + styles, including links) to a DOCX paragraph."""
    from docx.shared import Pt

    styles = run_data.get('styles', [])
    text = run_data.get('text', '')
    url = _link_url(styles)
    if url is not None:
        _add_hyperlink_docx(
            paragraph, text, url,
            bold='Bold' in styles or 'BoldItalic' in styles,
            italic='Italic' in styles or 'BoldItalic' in styles,
            code='Code' in styles,
            strike='Strikethrough' in styles,
            size=run_data.get('size'),
            font=run_data.get('font'),
        )
        return
    run = paragraph.add_run(text)
    # True or None, never False: an explicit "not bold" on every plain run
    # overrode the Heading styles' bold, so headings exported unbolded.
    run.bold = True if ('Bold' in styles or 'BoldItalic' in styles) else None
    run.italic = True if ('Italic' in styles or 'BoldItalic' in styles) else None
    if 'Code' in styles:
        run.font.name = 'Courier New'
        run.font.size = Pt(10)
    if 'Strikethrough' in styles:
        run.font.strike = True
    if 'Underline' in styles:
        run.font.underline = True
    # Character formatting on selected words (size, font) over the style's.
    if run_data.get('size'):
        run.font.size = Pt(float(run_data['size']))
    if run_data.get('font'):
        run.font.name = run_data['font']


def markdown_to_html(text: str) -> str:
    """Convert markdown text to HTML."""
    import markdown
    html = markdown.markdown(text, extensions=['fenced_code', 'tables', 'nl2br'])
    return html


def markdown_to_docx(text: str, output_path: str) -> bool:
    """Convert markdown to DOCX format with real formatting."""
    from docx.shared import Pt, Inches
    from docx.enum.text import WD_ALIGN_PARAGRAPH

    doc = _new_docx()
    for style_name in ['Normal'] + [f'Heading {level}' for level in range(1, 7)]:
        _set_style_complex_script_docx(doc.styles[style_name], 'Noto Sans Devanagari')

    # Set default font
    style = doc.styles['Normal']
    font = style.font
    font.name = 'Calibri'
    font.size = Pt(11)

    blocks = _parse_markdown_lines(text)

    for block in blocks:
        btype = block['type']

        if btype == 'blank':
            continue

        elif btype == 'heading':
            level = min(block['level'], 9)  # DOCX max heading level
            heading = doc.add_heading('', level=level)
            run = heading.add_run(block['text'])
            run.bold = True

        elif btype == 'paragraph':
            para = doc.add_paragraph()
            segments = _parse_inline(block['text'])
            for seg_text, styles in segments:
                run = para.add_run(seg_text)
                _apply_inline_docx(run, styles)

        elif btype == 'code_block':
            code_para = doc.add_paragraph()
            code_para.style = doc.styles['Normal']
            run = code_para.add_run(block['text'])
            run.font.name = 'Courier New'
            run.font.size = Pt(9)
            # Add light gray background via shading
            from docx.oxml.ns import qn
            from docx.oxml import OxmlElement
            shading = OxmlElement('w:shd')
            shading.set(qn('w:val'), 'clear')
            shading.set(qn('w:color'), 'auto')
            shading.set(qn('w:fill'), 'F5F5F5')
            run._element.get_or_add_rPr().append(shading)

        elif btype == 'blockquote':
            para = doc.add_paragraph()
            para.paragraph_format.left_indent = Inches(0.5)
            segments = _parse_inline(block['text'])
            for seg_text, styles in segments:
                run = para.add_run(seg_text)
                run.italic = True
                run.font.color.rgb = None  # default color

        elif btype == 'hr':
            doc.add_paragraph('─' * 50)

        elif btype == 'ul':
            for item in block['items']:
                para = doc.add_paragraph(style='List Bullet')
                segments = _parse_inline(item)
                for seg_text, styles in segments:
                    run = para.add_run(seg_text)
                    _apply_inline_docx(run, styles)

        elif btype == 'ol':
            for item in block['items']:
                para = doc.add_paragraph(style='List Number')
                segments = _parse_inline(item)
                for seg_text, styles in segments:
                    run = para.add_run(seg_text)
                    _apply_inline_docx(run, styles)

        elif btype == 'table':
            rows = block.get('rows', [])
            if rows:
                num_cols = max(len(r) for r in rows)
                table = doc.add_table(rows=len(rows), cols=num_cols)
                table.style = 'Table Grid'
                for ri, row in enumerate(rows):
                    for ci in range(num_cols):
                        cell_text = row[ci] if ci < len(row) else ''
                        cell_para = table.cell(ri, ci).paragraphs[0]
                        for seg_text, styles in _parse_inline(cell_text):
                            run = cell_para.add_run(seg_text)
                            _apply_inline_docx(run, styles)
                            if ri == 0:
                                run.bold = True

    os.makedirs(os.path.dirname(output_path) or '.', exist_ok=True)
    doc.save(output_path)
    return True


# ── PDF fonts ────────────────────────────────────────────────────────
#
# PDFs embed Sylph's bundled OFL fonts (assets/fonts) instead of the 14
# core PDF fonts, which are latin-1 only: Devanagari, arrows and dashes
# export as themselves, never as '?'. Rust passes the fonts folder in
# through `configure`, from its trusted-path logic.

_FONTS_DIR = None
# fpdf2 family ids (fpdf2 matches fallback fonts by the id without spaces).
_SERIF, _SANS, _MONO = 'EBGaramond', 'HankenGrotesk', 'JetBrainsMono'
# Family id -> {fpdf2 style: file in assets/fonts/<family>/}. A family with
# no true italic or bold uses its nearest face rather than failing.
_PDF_FONTS = {
    _SERIF: {'': 'Regular', 'I': 'Italic', 'B': 'Bold', 'BI': 'BoldItalic'},
    _SANS: {'': 'Regular', 'I': 'Italic', 'B': 'Bold', 'BI': 'BoldItalic'},
    _MONO: {'': 'Regular', 'I': 'Italic', 'B': 'Bold', 'BI': 'BoldItalic'},
    'SourceSerif4': {'': 'Regular', 'I': 'Italic', 'B': 'Bold', 'BI': 'BoldItalic'},
    'Lora': {'': 'Regular', 'I': 'Italic', 'B': 'Bold', 'BI': 'BoldItalic'},
    'Inter': {'': 'Regular', 'I': 'Italic', 'B': 'Bold', 'BI': 'BoldItalic'},
    'LiberationSerif': {'': 'Regular', 'I': 'Italic', 'B': 'Bold', 'BI': 'BoldItalic'},
    'LiberationSans': {'': 'Regular', 'I': 'Italic', 'B': 'Bold', 'BI': 'BoldItalic'},
    'LiberationMono': {'': 'Regular', 'I': 'Italic', 'B': 'Bold', 'BI': 'BoldItalic'},
    'NotoSerifDevanagari': {'': 'Regular', 'I': 'Regular', 'B': 'Bold', 'BI': 'Bold'},
    'NotoSansDevanagari': {'': 'Regular', 'I': 'Regular', 'B': 'Bold', 'BI': 'Bold'},
}
# Tried in order for characters the current font lacks.
# Source Serif 4 has the task boxes (☐ ☑) and Inter ✓ ✗ □, which the
# other families lack.
_PDF_FALLBACKS = (
    'NotoSerifDevanagari', 'NotoSansDevanagari', _SERIF, 'SourceSerif4', 'Inter', _SANS, _MONO,
)


def configure(fonts_dir=None):
    """Called by the Rust bridge with the bundled fonts folder."""
    global _FONTS_DIR
    _FONTS_DIR = fonts_dir


def _pdf_setup_fonts(pdf):
    """Register the bundled fonts, HarfBuzz shaping (Devanagari conjuncts)
    and the fallback chain on a new FPDF."""
    if not _FONTS_DIR or not os.path.isdir(_FONTS_DIR):
        raise RuntimeError(f"Sylph's fonts folder was not found ({_FONTS_DIR!r})")
    for family, styles in _PDF_FONTS.items():
        for style, face in styles.items():
            pdf.add_font(family, style, os.path.join(_FONTS_DIR, family, face + '.ttf'))
    pdf.set_text_shaping(True)
    pdf.set_fallback_fonts([family.lower() for family in _PDF_FALLBACKS])


def _strings(value):
    """Every string inside a JSON value (dict values, list items)."""
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for item in value.values():
            yield from _strings(item)
    elif isinstance(value, list):
        for item in value:
            yield from _strings(item)


def _pdf_output(pdf, output_path: str, source) -> List[str]:
    """Write the PDF and return warnings for characters of `source` (the
    exported text or JSON value) that no bundled font covers, which print
    as blank boxes, instead of hiding them. Returned, not stored: exports
    on other threads must not see each other's warnings."""
    os.makedirs(os.path.dirname(output_path) or '.', exist_ok=True)
    pdf.output(output_path)
    covered = set()
    for font in pdf.fonts.values():
        covered.update(getattr(font, 'cmap', None) or {})
    used = {ord(c) for text in _strings(source) for c in text}
    # Joiners and variation selectors have no glyph of their own.
    invisible = lambda c: c < 0x20 or chr(c).isspace() or c in (0x200C, 0x200D) or 0xFE00 <= c <= 0xFE0F
    lost = sorted(c for c in used - covered if not invisible(c))
    warnings = []
    if lost:
        warnings.append(
            'no bundled font has ' + ' '.join(f'{chr(c)} (U+{c:04X})' for c in lost[:10])
            + (f' and {len(lost) - 10} more' if len(lost) > 10 else '')
        )
    return warnings


def markdown_to_pdf(text: str, output_path: str) -> List[str]:
    """Convert markdown to PDF format with real formatting. Returns the
    export's warnings (empty when every character had a font)."""
    from fpdf import FPDF

    class SylphPDF(FPDF):
        def header(self):
            pass

        def footer(self):
            self.set_y(-15)
            self.set_font(_SANS, 'I', 8)
            self.set_text_color(128, 128, 128)
            self.cell(0, 10, f'Page {self.page_no()}/{{nb}}', align='C')

    pdf = SylphPDF()
    _pdf_setup_fonts(pdf)
    pdf.alias_nb_pages()
    pdf.set_auto_page_break(auto=True, margin=20)
    pdf.add_page()

    blocks = _parse_markdown_lines(text)

    for block in blocks:
        btype = block['type']

        if btype == 'blank':
            pdf.ln(4)
            continue

        elif btype == 'heading':
            level = block['level']
            size = max(24 - (level - 1) * 3, 12)
            pdf.set_font(_SANS, 'B', size)
            pdf.set_text_color(0, 0, 0)
            pdf.multi_cell(0, size * 0.5, block['text'])
            pdf.ln(2)

        elif btype == 'paragraph':
            _render_inline_pdf(pdf, block['text'])
            pdf.ln(3)

        elif btype == 'code_block':
            pdf.set_fill_color(245, 245, 245)
            pdf.set_font(_MONO, '', 9)
            pdf.set_text_color(50, 50, 50)
            code_text = block['text']
            # Escape special PDF characters
            code_text = code_text.replace('\\', '\\\\')
            pdf.multi_cell(0, 5, code_text, fill=True)
            pdf.ln(3)

        elif btype == 'blockquote':
            pdf.set_font(_SANS, 'I', 11)
            pdf.set_text_color(100, 100, 100)
            bx = pdf.l_margin + 10
            pdf.set_x(bx)
            pdf.multi_cell(pdf.w - pdf.r_margin - bx, 6, block['text'])
            pdf.set_text_color(0, 0, 0)
            pdf.ln(3)

        elif btype == 'hr':
            pdf.set_draw_color(180, 180, 180)
            y = pdf.get_y()
            pdf.line(20, y, pdf.w - 20, y)
            pdf.ln(5)

        elif btype == 'ul':
            for item in block['items']:
                pdf.set_font(_SANS, '', 11)
                pdf.set_text_color(0, 0, 0)
                pdf.set_x(pdf.l_margin)
                # Core PDF fonts are latin-1 only: use '-' instead of '•'.
                pdf.cell(5, 6, '-')
                _render_inline_pdf(pdf, item, indent=10)
                pdf.ln(1)
            pdf.ln(2)

        elif btype == 'ol':
            for idx, item in enumerate(block['items'], 1):
                pdf.set_font(_SANS, '', 11)
                pdf.set_text_color(0, 0, 0)
                pdf.set_x(pdf.l_margin)
                pdf.cell(8, 6, f'{idx}.')
                _render_inline_pdf(pdf, item, indent=13)
                pdf.ln(1)
            pdf.ln(2)

        elif btype == 'table':
            rows = block.get('rows', [])
            if rows:
                _pdf_table(pdf, rows)
                pdf.ln(2)

    return _pdf_output(pdf, output_path, text)


def _pdf_table(pdf, rows, col_widths=None):
    """Draw a table whose cells wrap instead of being cut short.

    `rows` are lists of plain strings and the first row is the header.
    fpdf2's table grows each row to its tallest cell and repeats the header
    after a page break, so no cell text is ever dropped. Text uses the
    document's body family, one point smaller than the body.
    """
    from fpdf.fonts import FontFace

    num_cols = max(len(r) for r in rows)
    if col_widths is not None and len(col_widths) != num_cols:
        col_widths = None  # equal widths rather than a mismatched layout
    _family, size, _spacing = _pdf_body(pdf)
    _pdf_font(pdf, '', -1)
    pdf.set_text_color(0, 0, 0)
    pdf.set_x(pdf.l_margin)
    with pdf.table(
        col_widths=col_widths,
        width=pdf.epw,
        text_align='LEFT',
        line_height=_pdf_line_height(size - 1, 1.3),
        headings_style=FontFace(emphasis='BOLD', fill_color=(230, 230, 230)),
    ) as table:
        for row in rows:
            cells = table.row()
            for ci in range(num_cols):
                cells.cell(row[ci] if ci < len(row) else '')


def _render_inline_pdf(pdf, text: str, indent: int = 0):
    """Render inline markdown text to PDF with formatting."""
    from fpdf import FPDF
    segments = _parse_inline(text)

    for seg_text, styles in segments:
        if not seg_text:
            continue
        if styles.get('code', False):
            pdf.set_font(_MONO, '', 10)
            pdf.set_text_color(200, 50, 50)
        elif styles.get('bold') and styles.get('italic'):
            pdf.set_font(_SANS, 'BI', 11)
            pdf.set_text_color(0, 0, 0)
        elif styles.get('bold'):
            pdf.set_font(_SANS, 'B', 11)
            pdf.set_text_color(0, 0, 0)
        elif styles.get('italic'):
            pdf.set_font(_SANS, 'I', 11)
            pdf.set_text_color(0, 0, 0)
        else:
            pdf.set_font(_SANS, '', 11)
            pdf.set_text_color(0, 0, 0)

        if indent > 0:
            # Reserve explicit width: multi_cell(w=0) after set_x() would
            # leave no room and raise "Not enough horizontal space".
            x = pdf.l_margin + indent
            w = pdf.w - pdf.r_margin - x
            pdf.set_x(x)
            pdf.multi_cell(w, 6, seg_text)
        else:
            pdf.set_x(pdf.l_margin)
            pdf.multi_cell(0, 6, seg_text)


# Portrait page sizes in points, mirroring sylph-core's PageSize::dimensions().
_PAGE_SIZES_PT = {'A4': (595.0, 842.0), 'Letter': (612.0, 792.0)}
# Sylph's default margins (PageMargins::default): 1 inch on every side.
_DEFAULT_MARGINS_PT = {'top': 72.0, 'bottom': 72.0, 'left': 72.0, 'right': 72.0}
_MM_PER_PT = 25.4 / 72.0


def _page_setup(doc_data: dict):
    """(width_pt, height_pt, margins_pt) of the document, landscape applied.

    Keys a JSON document lacks fall back to Sylph's own defaults (A4,
    1-inch margins, portrait), never to the export library's defaults,
    so the file matches the page the editor shows.
    """
    width, height = _PAGE_SIZES_PT.get(doc_data.get('page_size'), _PAGE_SIZES_PT['A4'])
    if doc_data.get('landscape'):
        width, height = height, width
    margins = dict(_DEFAULT_MARGINS_PT)
    margins.update(doc_data.get('page_margins') or {})
    return width, height, margins


def _apply_page_setup_docx(doc, doc_data: dict):
    """Give every section the document's page size, orientation, margins."""
    from docx.shared import Pt
    from docx.enum.section import WD_ORIENT

    width, height, margins = _page_setup(doc_data)
    for section in doc.sections:
        section.orientation = (
            WD_ORIENT.LANDSCAPE if doc_data.get('landscape') else WD_ORIENT.PORTRAIT
        )
        section.page_width = Pt(width)
        section.page_height = Pt(height)
        section.top_margin = Pt(margins['top'])
        section.bottom_margin = Pt(margins['bottom'])
        section.left_margin = Pt(margins['left'])
        section.right_margin = Pt(margins['right'])


# Heading (size, space before, space after) in points, mirroring
# heading_metrics_pt in apps/desktop/src/main.rs, so exports match the canvas.
_HEADING_METRICS_PT = {
    1: (28.0, 12.0, 6.0),
    2: (22.0, 10.0, 6.0),
    3: (18.0, 8.0, 4.0),
    4: (16.0, 6.0, 4.0),
    5: (14.0, 4.0, 4.0),
    6: (12.0, 4.0, 4.0),
}
# The canvas lays heading rows out at 1.4 × their size.
_HEADING_LINE_FACTOR = 1.4
# Body type for documents that omit it: sylph-core's Document::new().
_DEFAULT_BODY_FONT = 'EB Garamond'
_DEFAULT_BODY_SIZE = 11.0
_DEFAULT_LINE_SPACING = 1.15
# Paragraph gap when a block carries no style (ParagraphStyle's default).
_DEFAULT_SPACE_AFTER_PT = 8.0


def _body_type(doc_data: dict):
    """(font name, size in pt, line spacing) the canvas uses for body text."""
    name = doc_data.get('body_font') or _DEFAULT_BODY_FONT
    size = float(doc_data.get('body_font_size') or _DEFAULT_BODY_SIZE)
    spacing = float(doc_data.get('line_spacing') or _DEFAULT_LINE_SPACING)
    return name, size, spacing


def _heading_style(doc_data: dict, level) -> tuple:
    """(font name, line spacing or None) of the Heading `level` style: what
    its style sets, else the body font and no explicit spacing. Styles
    come from sylph-core's Document.heading_styles (index 0 = Heading 1)."""
    styles = doc_data.get('heading_styles') or []
    index = min(max(int(level), 1), 6) - 1
    style = styles[index] if index < len(styles) and isinstance(styles[index], dict) else {}
    font = style.get('font') or _body_type(doc_data)[0]
    return font, style.get('line_spacing')


def _heading_type(doc_data: dict, level) -> tuple:
    """(size pt, space before pt, space after pt) of the Heading `level`
    style: what its style sets, else the default scale (H1 28/12/6 …)."""
    size, before, after = _heading_metrics(level)
    styles = doc_data.get('heading_styles') or []
    index = min(max(int(level), 1), 6) - 1
    style = styles[index] if index < len(styles) and isinstance(styles[index], dict) else {}

    def pick(key, default):
        value = style.get(key)
        return default if value is None else float(value)

    return pick('size', size), pick('space_before', before), pick('space_after', after)


def _heading_metrics(level) -> tuple:
    return _HEADING_METRICS_PT[min(max(int(level), 1), 6)]


def _set_style_font_docx(style, name: str, size_pt: float):
    """Give a style an explicit font. The template's headings use theme
    fonts (w:asciiTheme and friends), which Word prefers over w:ascii, so
    those attributes are removed or the name would be ignored."""
    from docx.shared import Pt
    from docx.oxml.ns import qn

    from docx.oxml import OxmlElement
    from docx.shared import RGBColor

    style.font.name = name
    style.font.size = Pt(size_pt)
    # Black like the page, not the template's theme blue for headings.
    style.font.color.rgb = RGBColor(0, 0, 0)
    rpr = style.element.rPr
    # Complex-script text (Devanagari) at the same size as Latin text.
    size_cs = rpr.find(qn('w:szCs'))
    if size_cs is None:
        size_cs = OxmlElement('w:szCs')
        rpr.append(size_cs)
    size_cs.set(qn('w:val'), str(int(round(size_pt * 2))))
    rfonts = style.element.rPr.rFonts
    for attr in ('w:asciiTheme', 'w:hAnsiTheme', 'w:eastAsiaTheme', 'w:cstheme'):
        rfonts.attrib.pop(qn(attr), None)


def _new_docx():
    """A python-docx Document that Word opens as a current (2013+) file.
    The bundled template says Word 2010 (compatibilityMode 14), which Word
    shows as "[Compatibility Mode]" and lays out with 2010 rules."""
    from docx import Document
    from docx.oxml.ns import qn

    doc = Document()
    for setting in doc.settings.element.iter(qn('w:compatSetting')):
        if setting.get(qn('w:name')) == 'compatibilityMode':
            setting.set(qn('w:val'), '15')
    return doc


def _devanagari_family(font_name: str) -> str:
    """The Devanagari face that goes with the body typeface: serif with
    serif, sans with everything else (the same split as the PDF export)."""
    return 'Noto Serif Devanagari' if _pdf_family(font_name) == _SERIF else 'Noto Sans Devanagari'


def _set_style_complex_script_docx(style, cs_font: str):
    """Word and LibreOffice set Devanagari (a complex script) in the run's
    w:cs font, not w:ascii; without one they fall back to whatever the
    theme or system picks. The language marks it as Nepali for shaping
    and proofing."""
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    rpr = style.element.get_or_add_rPr()
    rfonts = rpr.get_or_add_rFonts()
    rfonts.set(qn('w:cs'), cs_font)
    rfonts.attrib.pop(qn('w:cstheme'), None)
    lang = rpr.find(qn('w:lang'))
    if lang is None:
        lang = OxmlElement('w:lang')
        rpr.append(lang)
    lang.set(qn('w:bidi'), 'ne-NP')


def _apply_typography_docx(doc, doc_data: dict):
    """Body text and headings in the document's typeface at the canvas's
    sizes, instead of the template's Calibri and theme heading fonts."""
    from docx.shared import Pt

    name, size, _spacing = _body_type(doc_data)
    cs_font = _devanagari_family(name)
    _set_style_font_docx(doc.styles['Normal'], name, size)
    _set_style_complex_script_docx(doc.styles['Normal'], cs_font)
    for level in range(1, 7):
        heading_size, before, after = _heading_type(doc_data, level)
        style = doc.styles[f'Heading {level}']
        heading_font, heading_spacing = _heading_style(doc_data, level)
        _set_style_font_docx(style, heading_font, heading_size)
        _set_style_complex_script_docx(style, _devanagari_family(heading_font))
        if heading_spacing:
            style.paragraph_format.line_spacing = heading_spacing
        style.font.bold = True
        style.font.italic = False
        style.paragraph_format.space_before = Pt(before)
        style.paragraph_format.space_after = Pt(after)


def _pdf_family(font_name: str) -> str:
    """The bundled family for the document's typeface: itself when it is
    bundled ("Source Serif 4" -> SourceSerif4); otherwise serif faces map
    to EB Garamond, monospace to JetBrains Mono and everything else to
    Hanken Grotesk (fonts that are not bundled cannot be embedded)."""
    # Word's standard fonts are drawn with their metric-compatible OFL
    # stand-ins (the DOCX keeps the Word name), as on the canvas.
    aliases = {'times new roman': 'LiberationSerif', 'arial': 'LiberationSans',
               'courier new': 'LiberationMono'}
    alias = aliases.get((font_name or '').strip().lower())
    if alias:
        return alias
    bundled = {family.lower(): family for family in _PDF_FONTS}
    exact = bundled.get((font_name or '').replace(' ', '').lower())
    if exact:
        return exact
    name = (font_name or '').lower()
    if 'mono' in name or 'courier' in name:
        return _MONO
    serif_faces = ('serif', 'garamond', 'georgia', 'times', 'cambria', 'palatino', 'baskerville')
    if 'sans' not in name and any(face in name for face in serif_faces):
        return _SERIF
    return _SANS


def _pdf_body(pdf):
    """(family, size pt, line spacing) that rich_pdf set on this PDF."""
    return (
        getattr(pdf, 'sylph_family', _SANS),
        getattr(pdf, 'sylph_size', _DEFAULT_BODY_SIZE),
        getattr(pdf, 'sylph_spacing', _DEFAULT_LINE_SPACING),
    )


def _pdf_line_height(size_pt: float, spacing: float) -> float:
    """Line height in mm for text of `size_pt`: size × spacing, like the canvas."""
    return size_pt * spacing * _MM_PER_PT


def _pdf_font(pdf, style: str = '', delta: float = 0.0):
    """The document's body family at body size + `delta` (captions -2,
    table text -1), so secondary text scales with the body."""
    family, size, _spacing = _pdf_body(pdf)
    pdf.set_font(family, style, size + delta)


def rich_docx(doc_json: str, output_path: str) -> bool:
    """Export a rich document (JSON) to DOCX format.

    The JSON structure:
    {
        "title": "...",
        "blocks": [
            {"CoverPage": {"data": {...}}},
            {"Heading": {"level": 1, "runs": [...]}},
            {"Paragraph": {"runs": [...], "style": {...}}},
            {"Image": {"data": {...}}},
            {"Table": {"data": {...}}},
            {"Caption": {"text": "..."}},
            {"HorizontalRule": null}
        ]
    }
    """
    import json
    from docx.shared import Pt, Inches, RGBColor
    from docx.enum.text import WD_ALIGN_PARAGRAPH

    doc_data = json.loads(doc_json)
    doc = _new_docx()
    # python-docx's template is Letter with 1.25" side margins; use the
    # document's own page setup instead.
    _apply_page_setup_docx(doc, doc_data)
    _apply_typography_docx(doc, doc_data)

    for block in doc_data.get('blocks', []):
        _render_block_docx(doc, block)

    os.makedirs(os.path.dirname(output_path) or '.', exist_ok=True)
    doc.save(output_path)
    return True


def _render_block_docx(doc, block: dict):
    """Render a single block to a DOCX document."""
    from docx.shared import Pt, Inches, RGBColor
    from docx.enum.text import WD_ALIGN_PARAGRAPH
    from docx.oxml.ns import qn
    from docx.oxml import OxmlElement

    if 'CoverPage' in block:
        _render_cover_page_docx(doc, block['CoverPage']['data'])
    elif 'Heading' in block:
        h = block['Heading']
        level = min(h['level'], 9)
        heading = doc.add_heading('', level=level)
        if _docx_alignment(h.get('alignment')) is not None:
            heading.alignment = _docx_alignment(h.get('alignment'))
        for run_data in h.get('runs', []):
            _add_styled_run_docx(heading, run_data)
    elif 'Paragraph' in block:
        p = block['Paragraph']
        para = doc.add_paragraph()
        style_data = p.get('style', {})
        # Always explicit: a value left unset falls back to the template's,
        # which is not what the page shows.
        para.paragraph_format.space_before = Pt(style_data.get('space_before', 0.0))
        para.paragraph_format.space_after = Pt(style_data.get('space_after', 8.0))
        para.paragraph_format.line_spacing = style_data.get('line_spacing', 1.15)
        if _docx_alignment(style_data.get('alignment')) is not None:
            para.alignment = _docx_alignment(style_data.get('alignment'))
        for run_data in p.get('runs', []):
            _add_styled_run_docx(para, run_data)
    elif 'Image' in block:
        img_data = block['Image']['data']
        path = img_data.get('path', '')

        def _image_caption():
            caption = img_data.get('caption')
            if caption:
                cap_para = doc.add_paragraph()
                cap_para.alignment = WD_ALIGN_PARAGRAPH.CENTER
                cap_run = cap_para.add_run(caption)
                cap_run.font.size = Pt(9)
                cap_run.font.italic = True
                cap_run.font.color.rgb = RGBColor(100, 100, 100)

        if path and os.path.exists(path):
            try:
                para = doc.add_paragraph()
                para.alignment = WD_ALIGN_PARAGRAPH.CENTER
                run = para.add_run()
                run.add_picture(path, width=Inches(min(img_data.get('width', 400) / 96, 6.0)))
                _image_caption()
            except Exception:
                doc.add_paragraph(f'[Image: {path}]')
                _image_caption()
        else:
            doc.add_paragraph(f'[Image: {path}]')
            _image_caption()
    elif 'Table' in block:
        table_data = block['Table']['data']
        rows = table_data.get('rows', [])
        if rows:
            num_cols = max(len(r) for r in rows)
            table = doc.add_table(rows=len(rows), cols=num_cols)
            table.style = 'Table Grid'
            # Column widths (percentages of the text width), as on the page.
            widths = table_data.get('column_widths') or []
            if len(widths) == num_cols:
                section = doc.sections[-1]
                text_width = section.page_width - section.left_margin - section.right_margin
                table.autofit = False
                for j, pct in enumerate(widths):
                    width = int(text_width * float(pct) / 100.0)
                    table.columns[j].width = width
                    for row_cells in table.rows:
                        row_cells.cells[j].width = width
            for i, row in enumerate(rows):
                for j, cell in enumerate(row):
                    if j < num_cols:
                        cell_para = table.cell(i, j).paragraphs[0]
                        for run_data in cell.get('runs', []):
                            _add_styled_run_docx(cell_para, run_data)
            caption = table_data.get('caption')
            if caption:
                cap_para = doc.add_paragraph()
                cap_run = cap_para.add_run(caption)
                cap_run.font.size = Pt(9)
                cap_run.font.italic = True
    elif 'Caption' in block:
        para = doc.add_paragraph()
        run = para.add_run(block['Caption']['text'])
        run.font.size = Pt(9)
        run.font.italic = True
        run.font.color.rgb = RGBColor(100, 100, 100)
    elif 'HorizontalRule' in block:
        doc.add_paragraph('─' * 50)
    elif 'PageBreak' in block:
        doc.add_page_break()
    elif 'List' in block:
        _render_list_docx(doc, block['List'])
    elif 'Quote' in block:
        _render_quote_docx(doc, block['Quote'])
    elif 'CodeBlock' in block:
        _render_code_block_docx(doc, block['CodeBlock'])
    else:
        # No-silent-drop law: an unrecognized block is a bug, not content
        # to quietly discard.
        raise ValueError(f'_render_block_docx: unknown block {block!r}')


def _render_list_docx(doc, list_data: dict):
    """Render list items with manual markers and hanging indentation.

    Numbering is derived per level (not Word numbering.xml) so DOCX, PDF
    and MD all agree on the exact same sequence.
    """
    from docx.shared import Inches, Pt

    items = list_data.get('items', [])
    counters = {}
    for item in items:
        level = min(int(item.get('level', 0)), 4)
        ordered = bool(item.get('ordered'))
        checked = item.get('checked')
        for deeper in [lv for lv in counters if lv > level]:
            del counters[deeper]
        if checked is not None:
            marker = '☑ ' if checked else '☐ '
        elif ordered:
            counters[level] = counters.get(level, 0) + 1
            marker = f'{counters[level]}. '
        else:
            counters.pop(level, None)
            marker = '• '
        para = doc.add_paragraph()
        para.paragraph_format.left_indent = Inches(0.3 * (level + 1))
        para.paragraph_format.space_after = Pt(3)
        para.add_run(marker)
        for run_data in item.get('runs', []):
            _add_styled_run_docx(para, run_data)


def _render_quote_docx(doc, quote_data: dict):
    """Render a blockquote as an indented paragraph with a left bar."""
    from docx.shared import Inches, Pt
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    level = min(int(quote_data.get('level', 1)), 4)
    para = doc.add_paragraph()
    para.paragraph_format.left_indent = Inches(0.25 * level + 0.15)
    para.paragraph_format.space_after = Pt(8)
    p_pr = para._p.get_or_add_pPr()
    p_bdr = OxmlElement('w:pBdr')
    left = OxmlElement('w:left')
    left.set(qn('w:val'), 'single')
    left.set(qn('w:sz'), '18')
    left.set(qn('w:space'), '8')
    left.set(qn('w:color'), 'AAAAAA')
    p_bdr.append(left)
    p_pr.append(p_bdr)
    for run_data in quote_data.get('runs', []):
        _add_styled_run_docx(para, run_data)
    # Gray italic, as the page draws quotes.
    from docx.shared import RGBColor
    for run in para.runs:
        run.italic = True
        run.font.color.rgb = RGBColor(0x50, 0x50, 0x50)


def _render_code_block_docx(doc, code_data: dict):
    """Render a fenced code block as shaded monospace with real line breaks."""
    from docx.shared import Pt
    from docx.oxml import OxmlElement
    from docx.oxml.ns import qn

    text = code_data.get('text', '')
    para = doc.add_paragraph()
    p_pr = para._p.get_or_add_pPr()
    shd = OxmlElement('w:shd')
    shd.set(qn('w:val'), 'clear')
    shd.set(qn('w:color'), 'auto')
    shd.set(qn('w:fill'), 'F2F2F2')
    p_pr.append(shd)
    para.paragraph_format.space_after = Pt(8)
    lines = text.split('\n')
    for idx, line in enumerate(lines):
        run = para.add_run(line)
        run.font.name = 'Courier New'
        run.font.size = Pt(9.5)
        if idx < len(lines) - 1:
            run.add_break()


def _render_cover_page_docx(doc, data: dict):
    """Render a cover page to DOCX."""
    from docx.shared import Pt, Inches, RGBColor
    from docx.enum.text import WD_ALIGN_PARAGRAPH

    template = data.get('template', 'Classic')

    # Add some spacing before title
    for _ in range(6):
        doc.add_paragraph('')

    # Title
    if data.get('title'):
        title_para = doc.add_paragraph()
        title_para.alignment = WD_ALIGN_PARAGRAPH.CENTER
        title_run = title_para.add_run(data['title'])
        title_run.bold = True
        if template == 'Bold':
            title_run.font.size = Pt(36)
        elif template == 'Modern':
            title_run.font.size = Pt(32)
        elif template == 'Academic':
            title_run.font.size = Pt(28)
        else:
            title_run.font.size = Pt(30)

    # Subtitle
    if data.get('subtitle'):
        sub_para = doc.add_paragraph()
        sub_para.alignment = WD_ALIGN_PARAGRAPH.CENTER
        sub_run = sub_para.add_run(data['subtitle'])
        sub_run.font.size = Pt(16)
        sub_run.font.color.rgb = RGBColor(100, 100, 100)

    # Spacing
    doc.add_paragraph('')
    doc.add_paragraph('')

    # Author
    if data.get('author'):
        author_para = doc.add_paragraph()
        author_para.alignment = WD_ALIGN_PARAGRAPH.CENTER
        author_run = author_para.add_run(data['author'])
        author_run.font.size = Pt(14)

    # Date
    if data.get('date'):
        date_para = doc.add_paragraph()
        date_para.alignment = WD_ALIGN_PARAGRAPH.CENTER
        date_run = date_para.add_run(data['date'])
        date_run.font.size = Pt(12)
        date_run.font.color.rgb = RGBColor(128, 128, 128)

    # Background image (if set and exists)
    bg_image = data.get('background_image')
    if bg_image and os.path.exists(bg_image):
        try:
            # Add as first-page image
            section = doc.sections[0]
            section.different_first_page_header_footer = True
            from docx.shared import Emu
            section.first_page_header.is_linked_to_previous = False
            # Just add the image at the top
            first_para = doc.paragraphs[0] if doc.paragraphs else doc.add_paragraph()
            first_para.alignment = WD_ALIGN_PARAGRAPH.CENTER
            run = first_para.add_run()
            run.add_picture(bg_image, width=Inches(6.0))
        except Exception:
            pass

    # Page break after cover
    doc.add_page_break()


def rich_pdf(doc_json: str, output_path: str) -> List[str]:
    """Export a rich document (JSON) to PDF format. Returns the export's
    warnings (empty when every character had a font)."""
    import json
    from fpdf import FPDF

    doc_data = json.loads(doc_json)
    # The document's page, not fpdf's default (A4 with 10 mm margins).
    width, height, margins = _page_setup(doc_data)
    bottom_mm = margins['bottom'] * _MM_PER_PT

    class SylphPDF(FPDF):
        def header(self):
            pass

        def footer(self):
            # Centre the 10 mm page-number cell in the bottom margin so it
            # never overlaps body text, whatever the margin preset.
            self.set_y(-(bottom_mm / 2 + 5))
            self.set_font(_SANS, 'I', 8)
            self.set_text_color(128, 128, 128)
            self.cell(0, 10, f'Page {self.page_no()}/{{nb}}', align='C')

    pdf = SylphPDF(unit='mm', format=(width * _MM_PER_PT, height * _MM_PER_PT))
    pdf.set_margins(
        margins['left'] * _MM_PER_PT, margins['top'] * _MM_PER_PT, margins['right'] * _MM_PER_PT
    )
    _pdf_setup_fonts(pdf)
    pdf.alias_nb_pages()
    pdf.set_auto_page_break(auto=True, margin=bottom_mm)
    # Body type for the block renderers (read back through _pdf_body).
    body_name, pdf.sylph_size, pdf.sylph_spacing = _body_type(doc_data)
    pdf.sylph_family = _pdf_family(body_name)
    pdf.sylph_doc = doc_data
    pdf.add_page()

    for block in doc_data.get('blocks', []):
        _render_block_pdf(pdf, block)

    return _pdf_output(pdf, output_path, doc_data)


# sylph-core Alignment (JSON omits Left) to fpdf2's text_align.
_PDF_ALIGN = {'Left': 'LEFT', 'Center': 'CENTER', 'Right': 'RIGHT', 'Justify': 'JUSTIFY'}


def _docx_alignment(name):
    """sylph-core Alignment to python-docx's, or None for Left (inherit)."""
    from docx.enum.text import WD_ALIGN_PARAGRAPH
    return {
        'Center': WD_ALIGN_PARAGRAPH.CENTER,
        'Right': WD_ALIGN_PARAGRAPH.RIGHT,
        'Justify': WD_ALIGN_PARAGRAPH.JUSTIFY,
    }.get(name)


def _write_pdf_runs(pdf, runs, spacing=None, default_style='', default_color=(0, 0, 0),
                    indent=0.0, bullet='', align='Left'):
    """Write inline runs as one wrapped paragraph and move below it.

    The runs go into a single fpdf2 text-flow paragraph, so lines wrap
    only between words even where the font changes mid-line (writing each
    run on its own broke words such as "sam|e" at the line end). Runs use
    the document's body family and size (see _pdf_body); plain runs use
    `default_style`/`default_color` (gray italic inside quotes); links are
    blue and clickable. `indent` (mm) shifts the paragraph right, and a
    `bullet` hangs in front of it, so wrapped lines align with the text.
    """
    family, size, body_spacing = _pdf_body(pdf)
    line_spacing = spacing or body_spacing
    runs = [r for r in runs if r.get('text')]
    _pdf_font(pdf, default_style)
    pdf.set_text_color(*default_color)
    with pdf.text_columns(
        l_margin=pdf.l_margin + indent,
        line_height=line_spacing,
        skip_leading_spaces=False,
    ) as columns:
        with columns.paragraph(
            text_align=_PDF_ALIGN.get(align, 'LEFT'),
            bullet_string=bullet,
            bullet_r_margin=1.5 if bullet else None,
        ) as paragraph:
            if not runs:
                paragraph.write(' ')
            for run_data in runs:
                styles = run_data.get('styles', [])
                strike = 'S' if 'Strikethrough' in styles else ''
                if 'Underline' in styles:
                    strike += 'U'
                url = _link_url(styles)
                # Character formatting on selected words overrides the
                # paragraph's size and family.
                run_size = float(run_data.get('size') or size)
                run_family = _pdf_family(run_data['font']) if run_data.get('font') else family
                if 'Code' in styles:
                    pdf.set_font(_MONO, strike, run_size - 1)
                    pdf.set_text_color(200, 50, 50)
                else:
                    if 'BoldItalic' in styles or ('Bold' in styles and 'Italic' in styles):
                        emphasis = 'BI'
                    elif 'Bold' in styles:
                        emphasis = 'B'
                    elif 'Italic' in styles:
                        emphasis = 'I'
                    else:
                        emphasis = default_style
                    pdf.set_font(run_family, emphasis + strike, run_size)
                    pdf.set_text_color(*(default_color if emphasis == default_style else (0, 0, 0)))
                if url is not None:
                    pdf.set_text_color(0, 90, 180)
                    paragraph.write(run_data['text'], link=url)
                else:
                    paragraph.write(run_data['text'])
    pdf.set_text_color(0, 0, 0)
    pdf.set_x(pdf.l_margin)


def _render_block_pdf(pdf, block: dict):
    """Render a single block to PDF."""
    if 'CoverPage' in block:
        _render_cover_page_pdf(pdf, block['CoverPage']['data'])
    elif 'Heading' in block:
        h = block['Heading']
        # The canvas's heading scale (H1 28 pt, 12 before / 6 after, …)
        # in the document's typeface.
        doc_data = getattr(pdf, 'sylph_doc', {})
        size, before, after = _heading_type(doc_data, h['level'])
        heading_font, heading_spacing = _heading_style(doc_data, h['level'])
        family = _pdf_family(heading_font)
        pdf.ln(before * _MM_PER_PT)
        pdf.set_x(pdf.l_margin)
        pdf.set_font(family, 'B', size)
        pdf.set_text_color(0, 0, 0)
        text = ''.join(r['text'] for r in h.get('runs', []))
        pdf.multi_cell(
            pdf.epw, _pdf_line_height(size, heading_spacing or _HEADING_LINE_FACTOR), text,
            align={'Center': 'C', 'Right': 'R', 'Justify': 'J'}.get(h.get('alignment'), 'L'),
        )
        pdf.ln(after * _MM_PER_PT)
    elif 'Paragraph' in block:
        p = block['Paragraph']
        style_data = p.get('style', {})
        _family, size, spacing = _pdf_body(pdf)
        spacing = style_data.get('line_spacing', spacing)
        # Spacing is in points, like every length in the model.
        space_before = style_data.get('space_before', 0.0)
        space_after = style_data.get('space_after', _DEFAULT_SPACE_AFTER_PT)
        # No space above a paragraph that starts a page, as in Word.
        if space_before and pdf.get_y() > pdf.t_margin + 0.01:
            pdf.ln(space_before * _MM_PER_PT)
        _write_pdf_runs(
            pdf, p.get('runs', []), spacing=spacing,
            align=style_data.get('alignment', 'Left'),
        )
        pdf.ln(space_after * _MM_PER_PT)
    elif 'Image' in block:
        img_data = block['Image']['data']
        path = img_data.get('path', '')
        if path and os.path.exists(path):
            try:
                w = min(img_data.get('width', 400) / 96 * 0.7, 170)
                pdf.set_x(pdf.l_margin)
                pdf.image(path, x=pdf.l_margin, w=w)
                caption = img_data.get('caption')
                if caption:
                    pdf.set_x(pdf.l_margin)
                    _pdf_font(pdf, 'I', -2)
                    pdf.set_text_color(100, 100, 100)
                    pdf.cell(pdf.epw, 5, caption, align='C')
                    pdf.ln(3)
                pdf.ln(3)
            except Exception:
                pdf.set_x(pdf.l_margin)
                _pdf_font(pdf, '', -1)
                pdf.cell(pdf.epw, 6, f'[Image: {path}]')
                pdf.ln(3)
                caption = img_data.get('caption')
                if caption:
                    pdf.set_x(pdf.l_margin)
                    _pdf_font(pdf, 'I', -2)
                    pdf.set_text_color(100, 100, 100)
                    pdf.cell(pdf.epw, 5, caption, align='C')
                    pdf.ln(3)
        else:
            pdf.set_x(pdf.l_margin)
            _pdf_font(pdf, '', -1)
            pdf.cell(pdf.epw, 6, f'[Image: {path}]')
            pdf.ln(3)
            caption = img_data.get('caption')
            if caption:
                pdf.set_x(pdf.l_margin)
                _pdf_font(pdf, 'I', -2)
                pdf.set_text_color(100, 100, 100)
                pdf.cell(pdf.epw, 5, caption, align='C')
                pdf.ln(3)
    elif 'Table' in block:
        _render_table_pdf(pdf, block['Table']['data'])
    elif 'Caption' in block:
        pdf.set_x(pdf.l_margin)
        _pdf_font(pdf, 'I', -2)
        pdf.set_text_color(100, 100, 100)
        pdf.cell(pdf.epw, 5, block['Caption']['text'])
        pdf.ln(3)
    elif 'HorizontalRule' in block:
        pdf.set_x(pdf.l_margin)
        pdf.set_draw_color(180, 180, 180)
        y = pdf.get_y()
        pdf.line(pdf.l_margin, y, pdf.w - pdf.r_margin, y)
        pdf.ln(5)
    elif 'PageBreak' in block:
        # Rust serializes the unit variant as the string "PageBreak";
        # `in` matches both the string and {"PageBreak": ...} shapes.
        pdf.add_page()
    elif 'List' in block:
        _render_list_pdf(pdf, block['List'])
    elif 'Quote' in block:
        _render_quote_pdf(pdf, block['Quote'])
    elif 'CodeBlock' in block:
        _render_code_block_pdf(pdf, block['CodeBlock'])
    else:
        # No-silent-drop law: unknown block shapes are bugs, not content.
        raise ValueError(f'_render_block_pdf: unknown block {block!r}')


def _render_list_pdf(pdf, list_data: dict):
    """Render list items with manual markers and per-level indentation."""
    items = list_data.get('items', [])
    counters = {}
    for item in items:
        level = min(int(item.get('level', 0)), 4)
        ordered = bool(item.get('ordered'))
        checked = item.get('checked')
        for deeper in [lv for lv in counters if lv > level]:
            del counters[deeper]
        if checked is not None:
            # Ballot boxes: in the fallback fonts, so no bundled face lacks them.
            marker = '☑' if checked else '☐'
        elif ordered:
            counters[level] = counters.get(level, 0) + 1
            marker = f'{counters[level]}.'
        else:
            counters.pop(level, None)
            marker = '•'
        # One item per paragraph, no gap between items; wrapped lines hang
        # under the text, not under the marker.
        _write_pdf_runs(pdf, item.get('runs', []), indent=6.0 * (level + 1), bullet=marker)
    pdf.ln(_DEFAULT_SPACE_AFTER_PT * _MM_PER_PT)


def _render_quote_pdf(pdf, quote_data: dict):
    """Render a blockquote as an indented, gray-italic paragraph."""
    level = min(int(quote_data.get('level', 1)), 4)
    _write_pdf_runs(
        pdf, quote_data.get('runs', []),
        default_style='I', default_color=(80, 80, 80), indent=6.0 * level,
    )
    pdf.ln(_DEFAULT_SPACE_AFTER_PT * _MM_PER_PT)


def _render_code_block_pdf(pdf, code_data: dict):
    """Render a fenced code block as indented monospace lines."""
    text = code_data.get('text', '')
    for line in text.split('\n'):
        pdf.set_x(pdf.l_margin + 6.0)
        pdf.set_font(_MONO, '', 9.5)
        pdf.set_text_color(40, 40, 40)
        pdf.multi_cell(pdf.epw - 6.0, 5, line or ' ')
    pdf.ln(4)


def _render_cover_page_pdf(pdf, data: dict):
    """Render a cover page to PDF: the template's sizes in the document's
    typeface, like the cover page on the canvas."""
    template = data.get('template', 'Classic')

    # Background image
    bg_image = data.get('background_image')
    if bg_image and os.path.exists(bg_image):
        try:
            pdf.image(bg_image, x=0, y=0, w=pdf.w, h=pdf.h)
        except Exception:
            pass

    pdf.ln(80)

    # Title
    if data.get('title'):
        if template == 'Bold':
            pdf.set_font(_pdf_body(pdf)[0], 'B', 36)
        elif template == 'Modern':
            pdf.set_font(_pdf_body(pdf)[0], 'B', 32)
        else:
            pdf.set_font(_pdf_body(pdf)[0], 'B', 30)
        pdf.set_text_color(0, 0, 0)
        pdf.set_x(pdf.l_margin)
        pdf.cell(pdf.epw, 15, data['title'], align='C')
        pdf.ln(15)

    # Subtitle
    if data.get('subtitle'):
        pdf.set_x(pdf.l_margin)
        pdf.set_font(_pdf_body(pdf)[0], '', 16)
        pdf.set_text_color(100, 100, 100)
        pdf.cell(pdf.epw, 10, data['subtitle'], align='C')
        pdf.ln(15)

    pdf.ln(20)

    # Author
    if data.get('author'):
        pdf.set_x(pdf.l_margin)
        pdf.set_font(_pdf_body(pdf)[0], '', 14)
        pdf.set_text_color(0, 0, 0)
        pdf.cell(pdf.epw, 10, data['author'], align='C')
        pdf.ln(10)

    # Date
    if data.get('date'):
        pdf.set_x(pdf.l_margin)
        pdf.set_font(_pdf_body(pdf)[0], '', 12)
        pdf.set_text_color(128, 128, 128)
        pdf.cell(pdf.epw, 10, data['date'], align='C')
        pdf.ln(10)

    pdf.add_page()


def _render_table_pdf(pdf, table_data: dict):
    """Render a table to PDF."""
    rows = table_data.get('rows', [])
    if not rows:
        return

    # Cell text is the concatenated runs; the model's column widths are
    # percentages, which fpdf2 takes as relative widths.
    text_rows = [
        [''.join(r.get('text', '') for r in cell.get('runs', [])) for cell in row]
        for row in rows
    ]
    _pdf_table(pdf, text_rows, col_widths=table_data.get('column_widths') or None)

    caption = table_data.get('caption')
    if caption:
        pdf.set_x(pdf.l_margin)
        _pdf_font(pdf, 'I', -2)
        pdf.set_text_color(100, 100, 100)
        pdf.cell(0, 5, caption, align='C')
        pdf.ln(3)

    pdf.ln(3)


def rich_markdown(doc_json: str, output_path: str) -> bool:
    """Export a rich document (JSON) to Markdown file."""
    import json

    doc_data = json.loads(doc_json)
    lines = []

    for block in doc_data.get('blocks', []):
        lines.extend(_render_block_markdown(block))

    os.makedirs(os.path.dirname(output_path) or '.', exist_ok=True)
    with open(output_path, 'w', encoding='utf-8') as f:
        f.write('\n'.join(lines))
    return True


def _md_escape_plain(text: str) -> str:
    """Backslash-escape parser-active markers so plain text round-trips.

    A literal `*` in a plain run came from an escaped input (`\\*`) or an
    unmatched marker; emitting it raw could pair with another run's marker
    and silently change emphasis on re-parse.
    """
    for ch in ('\\', '*', '`', '~'):
        text = text.replace(ch, '\\' + ch)
    return text


def _md_emphasis(run_data) -> str:
    """Wrap one run's text in its emphasis markers (no link handling)."""
    text = run_data.get('text', '')
    styles = run_data.get('styles', [])
    if ('Bold' in styles and 'Italic' in styles) or 'BoldItalic' in styles:
        return f'***{text}***'
    if 'Bold' in styles:
        return f'**{text}**'
    if 'Italic' in styles:
        return f'*{text}*'
    if 'Code' in styles:
        return f'`{text}`'
    if 'Strikethrough' in styles:
        return f'~~{text}~~'
    return _md_escape_plain(text)


def _md_runs(runs) -> str:
    """Render inline runs back to markdown (emphasis, code, links).

    Consecutive runs sharing one link target are emitted as a single
    `[...](url)` so `[nested **bold** inside](url)` doesn't split into
    three adjacent links.
    """
    parts = []
    i = 0
    while i < len(runs):
        url = _link_url(runs[i].get('styles', []))
        if url is not None:
            group = []
            while i < len(runs):
                if _link_url(runs[i].get('styles', [])) != url:
                    break
                group.append(_md_emphasis(runs[i]))
                i += 1
            parts.append('[' + ''.join(group) + f']({url})')
            continue
        parts.append(_md_emphasis(runs[i]))
        i += 1
    return ''.join(parts)


def _render_block_markdown(block: dict) -> list:
    """Render a single block to markdown lines."""
    if 'CoverPage' in block:
        data = block['CoverPage']['data']
        # Blank line after every field: consecutive cover lines would
        # re-parse as one paragraph and collapse on the next export, so
        # the output would not be a fixed point.
        fields = []
        if data.get('title'):
            fields.append(f'# {data["title"]}')
        if data.get('subtitle'):
            fields.append(f'*{data["subtitle"]}*')
        if data.get('author'):
            fields.append(f'**{data["author"]}**')
        if data.get('date'):
            fields.append(data['date'])
        lines = []
        for field in fields:
            lines.append(field)
            lines.append('')
        return lines
    elif 'Heading' in block:
        h = block['Heading']
        level = h['level']
        return [f'{"#" * level} {_md_runs(h.get("runs", []))}', '']
    elif 'Paragraph' in block:
        return [_md_runs(block['Paragraph'].get('runs', [])), '']
    elif 'Image' in block:
        img = block['Image']['data']
        alt = img.get('alt_text', 'image')
        path = img.get('path', '')
        lines = [f'![{alt}]({path})']
        if img.get('caption'):
            # The parser keeps captions as their own (italic) paragraph, so
            # a blank line must precede the caption — otherwise the next
            # export adds one and the output stops being a fixed point.
            lines.append('')
            lines.append(f'*{img["caption"]}*')
        lines.append('')
        return lines
    elif 'Table' in block:
        table = block['Table']['data']
        rows = table.get('rows', [])
        lines = []
        if rows:
            num_cols = max(len(r) for r in rows)
            for i, row in enumerate(rows):
                cells = []
                for j in range(num_cols):
                    if j < len(row):
                        # Cell styles must survive re-export exactly like
                        # List/Quote/Paragraph runs do — flattening to plain
                        # text would silently drop bold/links in tables.
                        cell = _md_runs(row[j].get('runs', []))
                        # An unescaped pipe would split this cell into extra
                        # columns when the output is parsed again.
                        cells.append(cell.replace('|', r'\|'))
                    else:
                        cells.append('')
                lines.append('| ' + ' | '.join(cells) + ' |')
                if i == 0:
                    lines.append('| ' + ' | '.join(['---'] * num_cols) + ' |')
        if table.get('caption'):
            # Captions re-parse as their own paragraph (see Image above):
            # emit a blank line before them so re-export is byte-identical.
            lines.append('')
            lines.append(f'*{table["caption"]}*')
        lines.append('')
        return lines
    elif 'Caption' in block:
        return [f'*{block["Caption"]["text"]}*', '']
    elif 'HorizontalRule' in block:
        return ['---', '']
    elif 'PageBreak' in block:
        # Layout instruction, not content: emit a pandoc-readable break.
        return ['\\newpage', '']
    elif 'List' in block:
        items = block['List'].get('items', [])
        lines = []
        counters = {}
        for item in items:
            level = min(int(item.get('level', 0)), 4)
            indent = '  ' * level
            checked = item.get('checked')
            ordered = bool(item.get('ordered'))
            for deeper in [lv for lv in counters if lv > level]:
                del counters[deeper]
            if checked is not None:
                prefix = f'{indent}- {"[x]" if checked else "[ ]"} '
            elif ordered:
                counters[level] = counters.get(level, 0) + 1
                prefix = f'{indent}{counters[level]}. '
            else:
                counters.pop(level, None)
                prefix = f'{indent}- '
            lines.append(prefix + _md_runs(item.get('runs', [])))
        lines.append('')
        return lines
    elif 'Quote' in block:
        q = block['Quote']
        level = min(int(q.get('level', 1)), 4)
        return [f'{">" * level} {_md_runs(q.get("runs", []))}', '']
    elif 'CodeBlock' in block:
        c = block['CodeBlock']
        body = c.get('text', '')
        # Widen the fence when the body itself contains backticks.
        fence = '````' if '```' in body else '```'
        lang = c.get('language', '')
        return [fence + lang, *body.split('\n'), fence, '']
    # No-silent-drop law: unknown block shapes are bugs, not content.
    raise ValueError(f'_render_block_markdown: unknown block {block!r}')
