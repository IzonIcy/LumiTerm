# Recording the demo

There is no `demo.mp4` in this repository yet. This page is the script for shooting it in one pass.

The recording was attempted on the machine that wrote this file and came out unusable. `screencapture -V 60` wrote a file whose first forty-five seconds are live and whose remaining fifteen repeat one frame. Window-scoped capture, full-screen capture, and capture with `caffeinate` holding the machine awake all froze at the same point. A video that stops before the last thing happens is worse than no video, so none was committed.

## What to show

The sidebar is the feature, so the video is one window and one key sequence. No slides, no narration. The window needs to be tall enough for three tabs and wide enough that the sidebar does not squeeze the grid, which means at least 900 by 600 points.

The point to make in sixty seconds: the panel knows which tab is running an agent without any shell integration, and it says so within one scan.

## Setup

Build a release binary so the window is smooth on camera:

```sh
cargo build --release
```

The sidebar matches the executable basename against `claude`, `codex`, `opencode`, `gemini`, `droid`, and `aider`. If the machine you record on has one of those installed and signed in, use it. If not, a stand-in with a matching name gives a repeatable take and needs no account:

```sh
mkdir -p ~/bin && cp /bin/sleep ~/bin/claude && export PATH="$HOME/bin:$PATH"
```

Start the app from a shell that has that `PATH`, or the stand-in will not be found:

```sh
./target/release/lumi-term
```

## The take

Run these in order. Timings assume the default three second scan interval.

1. **0:00** The window opens on `#1 Shell 1`. The sidebar lists one row and its state reads `none`.
2. **0:06** In the shell, run `claude`. Within one scan the row flips to `running` and the dot turns green. If you used the stand-in, the shell looks idle while the row says `running`, which is the whole claim: the scan sees the process, not the output.
3. **0:18** Press `Cmd+T`. A second tab appears, the sidebar grows a row, and the new row reads `none`.
4. **0:30** Switch back to tab one and exit the agent with `Ctrl+C`. The row reads `exited`. If the agent ran longer than ten seconds, a desktop notification fires at the same moment.
5. **0:42** Click the `▤` button in the title bar. The panel collapses and the grid takes the width back. Click it again to bring the panel back.
6. **0:52** Hold on the panel with one `exited` row and one `none` row so the last frame is readable.

Stop the capture after the hold. Do not close the window on camera, since the app quitting mid-frame looks like a crash.

## Shooting it

Record the window, not the screen. macOS wants a window id for that, and `screencapture` will not look one up, so save this as `/tmp/winid.swift` and pass the id:

```swift
import CoreGraphics
import Foundation
let needle = CommandLine.arguments[1]
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
for w in list where (w[kCGWindowOwnerName as String] as? String ?? "").contains(needle) {
    print(w[kCGWindowNumber as String] as? Int ?? 0)
}
```

```sh
swift /tmp/winid.swift Lumi-Term
screencapture -v -l"$(swift /tmp/winid.swift Lumi-Term | head -1)" demo.mov
```

The first command prints the id so you can see it resolved before recording. The window title is `Lumi-Term` from `[window].title` in the config, and the process is `lumi-term`, so match whichever of the two the window list reports.

## Checking the take before committing it

The file's duration says nothing about whether the frames are live. Pull frames and look at them:

```sh
for t in 4 10 20 32 44 54; do
    ffmpeg -loglevel error -ss "$t" -i demo.mov -frames:v 1 "frame-$t.png" -y
done
```

The frames must differ, the one at 20 seconds must show two sidebar rows, and the last one must show `exited` next to `none`. A repeated tail means the capture froze.

Trim and compress:

```sh
ffmpeg -i demo.mov -c:v libx264 -pix_fmt yuv420p -crf 28 -an demo.mp4
```

## Putting it in the README

Upload `demo.mp4` through the issue or pull request composer on GitHub, copy the `user-attachments` URL it returns, and paste that URL on its own line under the "Agent status sidebar" heading:

```markdown
https://github.com/user-attachments/assets/<id>
```

Then delete the sentence in that section that says there is no recording.
