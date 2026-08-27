# hyprgradient

**hyprgradient** generates your own gradient backgrounds for your Wayland desktop.

I really like gradient backgrounds. On my Windows 10 desktop PC I have a collection of PNGs that I just cycle through. I recently finally gave up on Windows 11 on my laptop and installed debian-gnome with hyprland as main environment for doing productivity things. So instead of relying on a bunch of images, I decided to write something something simple:

1) a generated wallpaper,
2) using a simple OpenGL shader,
3) using (multiple) scripted gradients,
4) in a simple config file.

## What is this?

hyprgradient is a small **Wayland** wallpaper generator that renders gradients using a GPU shader rather than generating the image on the CPU. It is primarily intended for use alongside **Hyprland**, although the project itself is really about generating wallpapers rather than being deeply tied to Hyprland as long as Wayland is used as the compositor. 

This is a small hobby project, might add things, might not.

## Configuration

By default, the config file is located at: 
```
~/.config/hypr/hyprgradient.conf
```

The config describes the gradients to generate, including things like colors (R,G,B), gradient type (LINEAR only for now), direction (360 degrees) and stops (max 32). Multiple gradients can be defined, so you can keep a small collection of backgrounds in a single config file. You can also turn gradients on and off by setting the active bool.

For example:

```
[[gradients]]
name = "midnight"
active = true
type = "linear"
direction = 45

  [[gradients.stops]]
  position = 0
  color = [15, 20, 40]

  [[gradients.stops]]
  position = 50
  color = [80, 20, 110]

  [[gradients.stops]]
  position = 100
  color = [130, 40, 170]

[[gradients]]
name = "sunset"
active = true
type = "linear"
direction = 0

  [[gradients.stops]]
  position = 0
  color = [255, 94, 77]

  [[gradients.stops]]
  position = 100
  color = [64, 12, 90]
```

The idea is that the config stays human-readable and easy to mess with. So I think it's pretty self explanatory too. Change a few gradients and colors in the .conf file, restart hyprgradient, and you have different wallpapers.

## Why?

Because I like gradients. :) And, honestly, this started as an evening experiment messing around with Rust, Wayland, OpenGL and a a few hours of vibe doodling later (particularly for the Wayland bit), there was a thing.

## License

0-FK-GVN: Feel free to mess with it, I don't care.
