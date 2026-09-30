"""
Sylph AI Module
Functions called from Rust via PyO3 bridge.

In v0.1.0, these are mocked stubs.
In v0.2.0, connect to Ollama or OpenAI API.
"""


def summarize(text: str) -> str:
    """Summarize the given text.

    TODO (v0.2.0):
    - Connect to Ollama: POST http://localhost:11434/api/generate
    - Or OpenAI: POST https://api.openai.com/v1/chat/completions
    - Use a local model like llama3 or phi3
    """
    # Mock implementation for v0.1.0
    words = text.split()
    if len(words) > 20:
        return f"Summary: {' '.join(words[:20])}..."
    return f"Summary: {text}"


def rewrite(text: str, style: str = "professional") -> str:
    """Rewrite text in a different style.

    TODO (v0.2.0):
    - Call LLM with system prompt based on style
    - Styles: professional, casual, academic, concise
    """
    return f"[{style}] {text}"


def chat_with_doc(question: str, context: str) -> str:
    """Answer a question about the given document context.

    TODO (v0.2.0):
    - RAG pipeline with embeddings
    - Or simple prompt engineering with context window
    """
    return f"Based on the document: {question}"
