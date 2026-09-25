# Sylph Markdown Kitchen Sink

This document exercises **every markdown construct** the editor parser supports. It is fed through the *same* parser the GUI uses, then exported to DOCX, PDF and Markdown.

## Inline styles

Plain text with *italic*, **bold**, ***bold-italic***, `inline code`, and ~~strikethrough~~. Snake_case stays plain, as does \*escaped asterisks\*.

Links come in [two flavors](https://example.com/docs) — inline with [nested **bold** inside](https://example.com/bold) — and as autolinks: <https://example.com/autolink>.

### Deep heading level three

#### Level four

##### Level five

###### Level six

## Lists

- First bullet
- Second bullet with **bold text**
  - Nested bullet
    - Deeper nesting
- Third bullet

1. Ordered first
2. Ordered second
   1. Nested ordered
   2. Nested ordered two
3. Ordered third

- [ ] Unchecked task
- [x] Checked task
  - [ ] Task inside a nested list

Wrapped items continue on the next line when indented, so this long bullet
stays part of the item.

## Blockquote

> A single-level quote with *emphasis* and a [link](https://example.com/quote).
> It continues across consecutive `>` lines.

>> A second-level quote, indented deeper than the first.

## Table

| Feature | Status | Notes |
| :--- | :---: | ---: |
| Lists | done | bullet, ordered, task |
| Quotes | done | nested levels |
| **Code** | `done` | fenced, verbatim |
| Links | done | inline and autolink |

## Code

```python
def greet(name):
    # markdown inside code is NOT parsed: **not bold**, # not a heading
    return f"Hello, {name}!"
```

```
plain fence, no language
```

## Thematic breaks

---

## Images

![A proof image](fixtures/proof.png)

Inline image in a sentence: ![icon](fixtures/proof.png) keeps the alt text.

## Final paragraph

Everything above must appear in every export format — nothing may be dropped.
