"""
Sylph Export Module
Document export functions called from Rust via PyO3.

Supports: DOCX, PDF, Markdown, HTML
"""

import os
import re
from typing import List, Tuple


def _parse_inline(text: str) -> List[Tuple[str, dict]]:
    """Parse inline markdown formatting into segments with styles.

    Returns list of (text, {'bold': bool, 'italic': bool, 'code': bool})
    """
    segments = []
    i = 0
    n = len(text)

    while i < n:
        # Bold + Italic: ***text*** or ___text___
        m = re.match(r'\*\*\*(.+?)\*\*\*', text[i:])
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
        while j < n and text[j] not in ('*', '_', '`', '['):
            j += 1
        segments.append((text[i:j], {'bold': False, 'italic': False, 'code': False}))
        i = j

    return segments if segments else [('', {'bold': False, 'italic': False, 'code': False})]


def _parse_markdown_lines(text: str) -> List[dict]:
    """Parse markdown text into structured blocks.

    Returns list of dicts with 'type' and content fields.
    Types: heading, paragraph, code_block, blockquote, hr, ul, ol, blank
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

        # Paragraph (collect consecutive non-special lines)
        para_lines = []
        while i < len(lines) and lines[i].strip() != '':
            # Stop if we hit a special block
            if (lines[i].strip().startswith('```') or
                lines[i].strip().startswith('#') or
                lines[i].strip().startswith('>') or
                re.match(r'^(\s*)[-*+]\s+', lines[i]) or
                re.match(r'^(\s*)\d+\.\s+', lines[i]) or
                re.match(r'^(\*\*\*+|---+|___+)\s*$', lines[i].strip())):
                break
            para_lines.append(lines[i])
            i += 1
        if para_lines:
            blocks.append({'type': 'paragraph', 'text': ' '.join(para_lines)})

    return blocks


def markdown_to_html(text: str) -> str:
    """Convert markdown text to HTML."""
    import markdown
    html = markdown.markdown(text, extensions=['fenced_code', 'tables', 'nl2br'])
    return html


def markdown_to_docx(text: str, output_path: str) -> bool:
    """Convert markdown to DOCX format with real formatting."""
    from docx import Document
    from docx.shared import Pt, Inches
    from docx.enum.text import WD_ALIGN_PARAGRAPH

    doc = Document()

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
                run.bold = styles.get('bold', False)
                run.italic = styles.get('italic', False)
                if styles.get('code', False):
                    run.font.name = 'Courier New'
                    run.font.size = Pt(10)

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
                    run.bold = styles.get('bold', False)
                    run.italic = styles.get('italic', False)

        elif btype == 'ol':
            for item in block['items']:
                para = doc.add_paragraph(style='List Number')
                segments = _parse_inline(item)
                for seg_text, styles in segments:
                    run = para.add_run(seg_text)
                    run.bold = styles.get('bold', False)
                    run.italic = styles.get('italic', False)

    os.makedirs(os.path.dirname(output_path) or '.', exist_ok=True)
    doc.save(output_path)
    return True


def markdown_to_pdf(text: str, output_path: str) -> bool:
    """Convert markdown to PDF format with real formatting."""
    from fpdf import FPDF

    class SylphPDF(FPDF):
        def header(self):
            pass

        def footer(self):
            self.set_y(-15)
            self.set_font('Helvetica', 'I', 8)
            self.set_text_color(128, 128, 128)
            self.cell(0, 10, f'Page {self.page_no()}/{{nb}}', align='C')

    pdf = SylphPDF()
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
            pdf.set_font('Helvetica', 'B', size)
            pdf.set_text_color(0, 0, 0)
            pdf.multi_cell(0, size * 0.5, block['text'])
            pdf.ln(2)

        elif btype == 'paragraph':
            _render_inline_pdf(pdf, block['text'])
            pdf.ln(3)

        elif btype == 'code_block':
            pdf.set_fill_color(245, 245, 245)
            pdf.set_font('Courier', '', 9)
            pdf.set_text_color(50, 50, 50)
            code_text = block['text']
            # Escape special PDF characters
            code_text = code_text.replace('\\', '\\\\')
            pdf.multi_cell(0, 5, code_text, fill=True)
            pdf.ln(3)

        elif btype == 'blockquote':
            pdf.set_font('Helvetica', 'I', 11)
            pdf.set_text_color(100, 100, 100)
            x = pdf.get_x()
            pdf.set_x(x + 10)
            pdf.multi_cell(0, 6, block['text'])
            pdf.set_text_color(0, 0, 0)
            pdf.ln(3)

        elif btype == 'hr':
            pdf.set_draw_color(180, 180, 180)
            y = pdf.get_y()
            pdf.line(20, y, pdf.w - 20, y)
            pdf.ln(5)

        elif btype == 'ul':
            for item in block['items']:
                pdf.set_font('Helvetica', '', 11)
                x = pdf.get_x()
                pdf.set_x(x + 5)
                pdf.cell(5, 6, chr(8226))  # bullet char
                _render_inline_pdf(pdf, item, indent=10)
                pdf.ln(1)
            pdf.ln(2)

        elif btype == 'ol':
            for idx, item in enumerate(block['items'], 1):
                pdf.set_font('Helvetica', '', 11)
                x = pdf.get_x()
                pdf.set_x(x + 5)
                pdf.cell(8, 6, f'{idx}.')
                _render_inline_pdf(pdf, item, indent=13)
                pdf.ln(1)
            pdf.ln(2)

    os.makedirs(os.path.dirname(output_path) or '.', exist_ok=True)
    pdf.output(output_path)
    return True


def _render_inline_pdf(pdf, text: str, indent: int = 0):
    """Render inline markdown text to PDF with formatting."""
    from fpdf import FPDF
    segments = _parse_inline(text)

    for seg_text, styles in segments:
        if styles.get('code', False):
            pdf.set_font('Courier', '', 10)
            pdf.set_text_color(200, 50, 50)
        elif styles.get('bold') and styles.get('italic'):
            pdf.set_font('Helvetica', 'BI', 11)
            pdf.set_text_color(0, 0, 0)
        elif styles.get('bold'):
            pdf.set_font('Helvetica', 'B', 11)
            pdf.set_text_color(0, 0, 0)
        elif styles.get('italic'):
            pdf.set_font('Helvetica', 'I', 11)
            pdf.set_text_color(0, 0, 0)
        else:
            pdf.set_font('Helvetica', '', 11)
            pdf.set_text_color(0, 0, 0)

        if indent > 0:
            x = pdf.get_x()
            pdf.set_x(x + indent)

        pdf.multi_cell(0, 6, seg_text)


def markdown_to_markdown(text: str, output_path: str) -> bool:
    """Save markdown text to a .md file (identity export)."""
    os.makedirs(os.path.dirname(output_path) or '.', exist_ok=True)
    with open(output_path, 'w', encoding='utf-8') as f:
        f.write(text)
    return True


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
    from docx import Document
    from docx.shared import Pt, Inches, RGBColor
    from docx.enum.text import WD_ALIGN_PARAGRAPH

    doc_data = json.loads(doc_json)
    doc = Document()

    # Set default font
    style = doc.styles['Normal']
    font = style.font
    font.name = 'Calibri'
    font.size = Pt(11)

    for block in doc_data.get('blocks', []):
        _render_block_docx(doc, block)

    os.makedirs(os.path.dirname(output_path) or '.', exist_ok=True)
    doc.save(output_path)
    return True


def _render_block_docx(doc, block: dict):
    """Render a single block to a DOCX document."""
    from docx.shared import Pt, Inches
    from docx.oxml.ns import qn
    from docx.oxml import OxmlElement

    if 'CoverPage' in block:
        _render_cover_page_docx(doc, block['CoverPage']['data'])
    elif 'Heading' in block:
        h = block['Heading']
        level = min(h['level'], 9)
        heading = doc.add_heading('', level=level)
        for run_data in h.get('runs', []):
            run = heading.add_run(run_data['text'])
            styles = run_data.get('styles', [])
            run.bold = 'Bold' in styles
            run.italic = 'Italic' in styles
    elif 'Paragraph' in block:
        p = block['Paragraph']
        para = doc.add_paragraph()
        style_data = p.get('style', {})
        if style_data.get('space_before', 0) > 0:
            para.paragraph_format.space_before = Pt(style_data['space_before'])
        if style_data.get('space_after', 0) > 0:
            para.paragraph_format.space_after = Pt(style_data['space_after'])
        if style_data.get('line_spacing', 1.5) != 1.0:
            para.paragraph_format.line_spacing = style_data.get('line_spacing', 1.5)
        for run_data in p.get('runs', []):
            run = para.add_run(run_data['text'])
            styles = run_data.get('styles', [])
            run.bold = 'Bold' in styles or 'BoldItalic' in styles
            run.italic = 'Italic' in styles or 'BoldItalic' in styles
            if 'Code' in styles:
                run.font.name = 'Courier New'
                run.font.size = Pt(10)
            if 'Strikethrough' in styles:
                run.font.strike = True
            if 'Underline' in styles:
                run.font.underline = True
    elif 'Image' in block:
        img_data = block['Image']['data']
        path = img_data.get('path', '')
        if path and os.path.exists(path):
            try:
                para = doc.add_paragraph()
                para.alignment = WD_ALIGN_PARAGRAPH.CENTER
                run = para.add_run()
                run.add_picture(path, width=Inches(min(img_data.get('width', 400) / 96, 6.0)))
                caption = img_data.get('caption')
                if caption:
                    cap_para = doc.add_paragraph()
                    cap_para.alignment = WD_ALIGN_PARAGRAPH.CENTER
                    cap_run = cap_para.add_run(caption)
                    cap_run.font.size = Pt(9)
                    cap_run.font.italic = True
                    cap_run.font.color.rgb = RGBColor(100, 100, 100)
            except Exception:
                doc.add_paragraph(f'[Image: {path}]')
        else:
            doc.add_paragraph(f'[Image: {path}]')
    elif 'Table' in block:
        table_data = block['Table']['data']
        rows = table_data.get('rows', [])
        if rows:
            num_cols = max(len(r) for r in rows)
            table = doc.add_table(rows=len(rows), cols=num_cols)
            table.style = 'Table Grid'
            for i, row in enumerate(rows):
                for j, cell in enumerate(row):
                    if j < num_cols:
                        cell_para = table.cell(i, j).paragraphs[0]
                        for run_data in cell.get('runs', []):
                            run = cell_para.add_run(run_data['text'])
                            styles = run_data.get('styles', [])
                            run.bold = 'Bold' in styles
                            run.italic = 'Italic' in styles
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


def rich_pdf(doc_json: str, output_path: str) -> bool:
    """Export a rich document (JSON) to PDF format."""
    import json
    from fpdf import FPDF

    doc_data = json.loads(doc_json)

    class SylphPDF(FPDF):
        def header(self):
            pass

        def footer(self):
            self.set_y(-15)
            self.set_font('Helvetica', 'I', 8)
            self.set_text_color(128, 128, 128)
            self.cell(0, 10, f'Page {self.page_no()}/{{nb}}', align='C')

    pdf = SylphPDF()
    pdf.alias_nb_pages()
    pdf.set_auto_page_break(auto=True, margin=20)
    pdf.add_page()

    for block in doc_data.get('blocks', []):
        _render_block_pdf(pdf, block)

    os.makedirs(os.path.dirname(output_path) or '.', exist_ok=True)
    pdf.output(output_path)
    return True


def _render_block_pdf(pdf, block: dict):
    """Render a single block to PDF."""
    if 'CoverPage' in block:
        _render_cover_page_pdf(pdf, block['CoverPage']['data'])
    elif 'Heading' in block:
        h = block['Heading']
        level = h['level']
        size = max(24 - (level - 1) * 3, 12)
        pdf.set_x(pdf.l_margin)
        pdf.set_font('Helvetica', 'B', size)
        pdf.set_text_color(0, 0, 0)
        text = ''.join(r['text'] for r in h.get('runs', []))
        pdf.multi_cell(pdf.epw, size * 0.5, text)
        pdf.ln(2)
    elif 'Paragraph' in block:
        p = block['Paragraph']
        style_data = p.get('style', {})
        space_after = style_data.get('space_after', 3)
        for run_data in p.get('runs', []):
            text = run_data['text']
            styles = run_data.get('styles', [])
            if 'Code' in styles:
                pdf.set_font('Courier', '', 10)
                pdf.set_text_color(200, 50, 50)
            elif 'Bold' in styles and 'Italic' in styles:
                pdf.set_font('Helvetica', 'BI', 11)
                pdf.set_text_color(0, 0, 0)
            elif 'Bold' in styles:
                pdf.set_font('Helvetica', 'B', 11)
                pdf.set_text_color(0, 0, 0)
            elif 'Italic' in styles:
                pdf.set_font('Helvetica', 'I', 11)
                pdf.set_text_color(0, 0, 0)
            else:
                pdf.set_font('Helvetica', '', 11)
                pdf.set_text_color(0, 0, 0)
            pdf.set_x(pdf.l_margin)
            pdf.multi_cell(pdf.epw, 6, text)
        pdf.ln(space_after)
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
                    pdf.set_font('Helvetica', 'I', 9)
                    pdf.set_text_color(100, 100, 100)
                    pdf.cell(pdf.epw, 5, caption, align='C')
                    pdf.ln(3)
                pdf.ln(3)
            except Exception:
                pdf.set_x(pdf.l_margin)
                pdf.set_font('Helvetica', '', 10)
                pdf.cell(pdf.epw, 6, f'[Image: {path}]')
                pdf.ln(3)
        else:
            pdf.set_x(pdf.l_margin)
            pdf.set_font('Helvetica', '', 10)
            pdf.cell(pdf.epw, 6, f'[Image: {path}]')
            pdf.ln(3)
    elif 'Table' in block:
        _render_table_pdf(pdf, block['Table']['data'])
    elif 'Caption' in block:
        pdf.set_x(pdf.l_margin)
        pdf.set_font('Helvetica', 'I', 9)
        pdf.set_text_color(100, 100, 100)
        pdf.cell(pdf.epw, 5, block['Caption']['text'])
        pdf.ln(3)
    elif 'HorizontalRule' in block:
        pdf.set_x(pdf.l_margin)
        pdf.set_draw_color(180, 180, 180)
        y = pdf.get_y()
        pdf.line(pdf.l_margin, y, pdf.w - pdf.r_margin, y)
        pdf.ln(5)


def _render_cover_page_pdf(pdf, data: dict):
    """Render a cover page to PDF."""
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
            pdf.set_font('Helvetica', 'B', 36)
        elif template == 'Modern':
            pdf.set_font('Helvetica', 'B', 32)
        else:
            pdf.set_font('Helvetica', 'B', 30)
        pdf.set_text_color(0, 0, 0)
        pdf.set_x(pdf.l_margin)
        pdf.cell(pdf.epw, 15, data['title'], align='C')
        pdf.ln(15)

    # Subtitle
    if data.get('subtitle'):
        pdf.set_x(pdf.l_margin)
        pdf.set_font('Helvetica', '', 16)
        pdf.set_text_color(100, 100, 100)
        pdf.cell(pdf.epw, 10, data['subtitle'], align='C')
        pdf.ln(15)

    pdf.ln(20)

    # Author
    if data.get('author'):
        pdf.set_x(pdf.l_margin)
        pdf.set_font('Helvetica', '', 14)
        pdf.set_text_color(0, 0, 0)
        pdf.cell(pdf.epw, 10, data['author'], align='C')
        pdf.ln(10)

    # Date
    if data.get('date'):
        pdf.set_x(pdf.l_margin)
        pdf.set_font('Helvetica', '', 12)
        pdf.set_text_color(128, 128, 128)
        pdf.cell(pdf.epw, 10, data['date'], align='C')
        pdf.ln(10)

    pdf.add_page()


def _render_table_pdf(pdf, table_data: dict):
    """Render a table to PDF."""
    rows = table_data.get('rows', [])
    if not rows:
        return

    num_cols = max(len(r) for r in rows)
    col_width = (pdf.w - pdf.l_margin - pdf.r_margin) / num_cols

    pdf.set_font('Helvetica', '', 10)
    for i, row in enumerate(rows):
        pdf.set_x(pdf.l_margin)
        for j, cell in enumerate(row):
            if j < num_cols:
                text = ''.join(r.get('text', '') for r in cell.get('runs', []))
                is_header = i == 0
                if is_header:
                    pdf.set_font('Helvetica', 'B', 10)
                    pdf.set_fill_color(230, 230, 230)
                else:
                    pdf.set_font('Helvetica', '', 10)
                    pdf.set_fill_color(255, 255, 255)
                # Use explicit width and ensure x position
                pdf.cell(col_width, 8, text[:50], border=1, fill=is_header)
        pdf.ln()

    caption = table_data.get('caption')
    if caption:
        pdf.set_x(pdf.l_margin)
        pdf.set_font('Helvetica', 'I', 9)
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


def _render_block_markdown(block: dict) -> list:
    """Render a single block to markdown lines."""
    if 'CoverPage' in block:
        data = block['CoverPage']['data']
        lines = []
        if data.get('title'):
            lines.append(f'# {data["title"]}')
        if data.get('subtitle'):
            lines.append(f'*{data["subtitle"]}*')
        if data.get('author'):
            lines.append(f'**{data["author"]}**')
        if data.get('date'):
            lines.append(data['date'])
        lines.append('')
        return lines
    elif 'Heading' in block:
        h = block['Heading']
        level = h['level']
        text = ''.join(r['text'] for r in h.get('runs', []))
        return [f'{"#" * level} {text}', '']
    elif 'Paragraph' in block:
        p = block['Paragraph']
        parts = []
        for run_data in p.get('runs', []):
            text = run_data['text']
            styles = run_data.get('styles', [])
            if 'Bold' in styles and 'Italic' in styles:
                parts.append(f'***{text}***')
            elif 'Bold' in styles:
                parts.append(f'**{text}**')
            elif 'Italic' in styles:
                parts.append(f'*{text}*')
            elif 'Code' in styles:
                parts.append(f'`{text}`')
            elif 'Strikethrough' in styles:
                parts.append(f'~~{text}~~')
            else:
                parts.append(text)
        return [''.join(parts), '']
    elif 'Image' in block:
        img = block['Image']['data']
        alt = img.get('alt_text', 'image')
        path = img.get('path', '')
        lines = [f'![{alt}]({path})']
        if img.get('caption'):
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
                        text = ''.join(r.get('text', '') for r in row[j].get('runs', []))
                        cells.append(text)
                    else:
                        cells.append('')
                lines.append('| ' + ' | '.join(cells) + ' |')
                if i == 0:
                    lines.append('| ' + ' | '.join(['---'] * num_cols) + ' |')
        if table.get('caption'):
            lines.append(f'*{table["caption"]}*')
        lines.append('')
        return lines
    elif 'Caption' in block:
        return [f'*{block["Caption"]["text"]}*', '']
    elif 'HorizontalRule' in block:
        return ['---', '']
    return []
