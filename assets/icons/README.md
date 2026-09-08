# Sylph Logo Assets

Place your logo files here:

- `sylph.png` - Main app icon (512x512 recommended)
- `sylph.ico` - Windows icon
- `sylph.svg` - Scalable vector

## Usage

The GPUI app loads the icon from this directory:
```rust
cx.set_window_icon(Some(ImageSource::Data(Cow::Owned(
    std::fs::read("assets/icons/sylph.png").unwrap()
))));
```
