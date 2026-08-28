#!/usr/bin/env python3
#
# Checks that the map does not lose its picture as it is zoomed.
#
#   ./tests/zoom-sweep.py http://127.0.0.1:8080
#   ./tests/zoom-sweep.py http://127.0.0.1:8080 --at 512000,512000
#
# Four times now the map has gone blank past some zoom, each time for a different
# reason — a tile layer's inherited maximum, a level nothing had built, one pixel
# per block sitting at a floating zoom, a palette with no colours left in it. Every
# one of them looked the same from the page: a flat dark field, indistinguishable
# from ground nobody has explored. None was catchable by testing the viewer's
# arithmetic, because in each case the arithmetic was right.
#
# So this tests the only thing they had in common. It drives a real browser across
# the zoom range and counts how many distinct colours reach the screen. Terrain is
# thousands; an empty screen is a handful. A step that falls off a cliff the ones
# either side of it did not is the failure, whatever caused it.
#
# Needs the map service running with a world worth looking at, and a chromium.
# Prints a line per step and exits non-zero on a cliff, so it composes.

import argparse
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# One pixel per block is the boundary the stored levels meet the drawn one at, so
# the steps crowd around it — that is where three of the four went wrong.
STEPS = [0.05, 0.1, 0.25, 0.5, 0.6, 0.66, 0.7, 0.75, 0.9, 1, 1.5, 2, 4, 8]

# Below this a screen is bare ground and furniture, with no terrain on it. Real
# terrain runs to five figures, so anything near the floor is unambiguous.
DETAIL_FLOOR = 1000

# What counts as falling off a cliff rather than simply getting coarser: zooming
# in never has less to show than the step before it, give or take.
CLIFF = 4


def browser():
    """Whatever chromium this machine has, native or flatpak."""
    for name in ("chromium", "chromium-browser", "google-chrome", "chrome"):
        if found := shutil.which(name):
            return [found]
    if shutil.which("flatpak"):
        listed = subprocess.run(
            ["flatpak", "list", "--app", "--columns=application"],
            capture_output=True, text=True, check=False).stdout
        for line in listed.splitlines():
            if "chromium" in line.lower() or "chrome" in line.lower():
                return ["flatpak", "run", "--filesystem=/tmp", line.strip()]
    return None


def colours(shot):
    """How many distinct colours a screenshot holds."""
    from PIL import Image
    with Image.open(shot) as image:
        return len(set(image.convert("RGB").get_flattened_data()))


def look(run, url, into):
    subprocess.run(
        run + ["--headless", "--disable-gpu", "--no-sandbox",
               "--window-size=1280,900", "--virtual-time-budget=10000",
               f"--screenshot={into}", url],
        capture_output=True, check=False)
    return colours(into) if Path(into).exists() else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("url", help="where the map service is listening")
    parser.add_argument("--at", default="", metavar="X,Z",
                        help="centre on these world coordinates (default: the map's own)")
    asked = parser.parse_args()

    run = browser()
    if run is None:
        print("zoom-sweep: no chromium found — skipping", file=sys.stderr)
        return 0

    seen = []
    with tempfile.TemporaryDirectory(dir="/tmp") as scratch:
        for step in STEPS:
            where = f"{asked.url}/#{asked.at},{step}" if asked.at else f"{asked.url}/#{step}"
            count = look(run, where, f"{scratch}/{step}.png")
            seen.append((step, count))
            print(f"  {step:>6} px/block  {count:>7} colours")

    drawn = [(step, count) for step, count in seen if count >= DETAIL_FLOOR]
    if not drawn:
        print("zoom-sweep: nothing was drawn at any zoom — is the world exported?",
              file=sys.stderr)
        return 1

    failed = False
    for (was, before), (now, after) in zip(seen, seen[1:]):
        if before >= DETAIL_FLOOR and after * CLIFF < before:
            print(f"zoom-sweep: the map falls off between {was} and {now} px/block "
                  f"({before} colours to {after})", file=sys.stderr)
            failed = True

    print("zoom-sweep: " + ("failed" if failed else "the picture holds at every zoom"))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
