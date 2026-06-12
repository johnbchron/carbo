# `carbo`

> "Wanderer, there is no road, the road is made by walking." – Antonio Machado

I'm writing a new terminal emulator, for reasons unjustifiable.

Watch the process here.

## Acknowledgments

I've built very few foundational pieces of this application. It's mostly a few robust pieces wired together:
- `portable-pty`: x-platform pty opening (thank you @wezterm)
- `vello`: drawing stack (thank you @linebender)
- `vt100`: vt100 grid and state machine (thank you @doy)
  - uses `vte`: vt100 parsing and escaping (thank you @alacritty)
- `wgpu`: rendering & gpu stack (thank you @gfx-rs)
- `winit`: windowing (thank you @rust-windowing)

---

Please note that this is a hobby project and a labor of love, not a product.
It is an exercise in craft, and as such I have no desire to use artificial
intelligence in the process.
