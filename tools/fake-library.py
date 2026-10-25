#!/usr/bin/env python3
"""Builds a fake game library for screenshots and manual tests of the shell.

    python3 tools/fake-library.py <out>
    LOCALAPPDATA=<out>/appdata CANAPE_LIBRARY_FIXTURE=<out>/library \\
        ./target/debug/canape-shell

Every game is made up and every cover is drawn here (ImageMagick `convert`),
so the pictures can go in the documentation. The Epic and GOG games get
their covers in `<out>/appdata/Canape/covers-fixture`, where the shell keeps
downloaded covers: they show what the online covers look like. Nine more
games are owned but not installed (`<out>/library/owned.json`), for the
library's "To install" mode; A there runs a pretend one-minute install.
Two games no store knows are in `<out>/library/games.json` (one with its
own cover, one without), and `<out>/library/shortcuts` holds shortcuts
for Settings -> Add a game (one is an uninstaller, which is left out).
Three Steam games have a time to beat (`igdb-fixture.json`), shown below
the fold of their game page, and two have achievements
(`achievements-fixture.json`, icons drawn in `achievement-icons-fixture`).
"""

import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
import time

DAY = 86400

# (store, title, two gradient colours, days since last played or None)
GAMES = [
    ("steam", "Ashen Tide", "#1e3a8a", "#f97316", 0),
    ("steam", "Neon Courier", "#581c87", "#06b6d4", 1),
    ("epic", "Starfall Tactics", "#0f172a", "#a855f7", 2),
    ("xbox", "Contoso Racing", "#7f1d1d", "#facc15", 3),
    ("steam", "Hollow Lantern", "#052e16", "#84cc16", 5),
    ("gog", "Moonlit Archive", "#1e1b4b", "#93c5fd", 9),
    ("steam", "Skyward Drift", "#0c4a6e", "#fde68a", 16),
    ("epic", "Glass Harbor", "#134e4a", "#5eead4", 30),
    ("xbox", "Fabrikam Quest", "#3f1d38", "#fb7185", 45),
    ("steam", "Iron Meridian", "#292524", "#d6d3d1", 80),
    ("gog", "Copper Crown", "#431407", "#fdba74", None),
    ("epic", "Velvet Circuit", "#4a044e", "#f0abfc", None),
    ("steam", "Pixel Harvest", "#14532d", "#fef08a", None),
    ("xbox", "Northwind Legends", "#172554", "#e0f2fe", None),
]


# Games of games.json: (title, executable, cover colours or None).
EXTERNAL = [
    ("Lantern Keeper", "D:\\Games\\Lantern Keeper\\Lantern.exe", ("#422006", "#fcd34d")),
    ("Orbit Golf", "D:\\Itch\\orbit-golf\\OrbitGolf.exe", None),
]

# Start menu shortcuts: (name, target).
SHORTCUTS = [
    ("Tiny Rally", "D:\\Games\\Tiny Rally\\TinyRally.exe"),
    ("Uninstall Tiny Rally", "D:\\Games\\Tiny Rally\\unins000.exe"),
    ("Mesa Drift", "D:\\Itch\\mesa-drift\\MesaDrift.exe"),
    ("Lantern Keeper", "D:\\Games\\Lantern Keeper\\Lantern.exe"),
]

# Size once installed of the owned Epic and GOG games, in bytes (Lumen Drift
# does not fit in the shell's fake 48.2 GB free).
OWNED_SIZES = {
    "Quiet Orbit": 12_400_000_000,
    "Lumen Drift": 63_100_000_000,
    "Mosaic Run": 7_900_000_000,
    "Lantern Keep": 3_200_000_000,
    "Saltmarsh": 21_700_000_000,
}

# Owned but not installed: (store, title, colours, store id).
OWNED = [
    ("steam", "Tidal Bloom", "#0e7490", "#fef3c7", 910001),
    ("epic", "Quiet Orbit", "#1e293b", "#c4b5fd", "QuietOrbit"),
    ("steam", "Paper Kingdom", "#7c2d12", "#fde68a", 910002),
    ("epic", "Lumen Drift", "#064e3b", "#a7f3d0", "LumenDrift"),
    ("steam", "Rust Garden", "#3f3f46", "#fca5a5", 910003),
    ("steam", "Northbound", "#1e3a8a", "#e0e7ff", 910004),
    ("epic", "Mosaic Run", "#701a75", "#fbcfe8", "MosaicRun"),
    ("gog", "Lantern Keep", "#422006", "#fdba74", "1900000001"),
    ("gog", "Saltmarsh", "#0c4a6e", "#bae6fd", "1900000002"),
]


def cover(path, title, top, bottom, size="600x900", titled=True):
    """A gradient, two soft discs and, unless `titled` is false, the title."""
    width, height = (int(v) for v in size.split("x"))
    words = title.upper().split()
    text = [
        "-font", "DejaVu-Sans-Bold", "-fill", "white",
        "-pointsize", str(int(width / 9)), "-gravity", "south",
        "-annotate", f"+0+{int(height * 0.12)}", "\n".join(words),
    ] if titled else []
    subprocess.run(
        [
            "convert", "-size", size, f"gradient:{top}-{bottom}",
            "-fill", "#ffffff22", "-draw", f"circle {width * 0.7},{height * 0.3} {width * 1.05},{height * 0.3}",
            "-fill", "#00000033", "-draw", f"circle {width * 0.2},{height * 0.75} {width * 0.55},{height * 0.75}",
            *text,
            "-quality", "88", path,
        ],
        check=True,
    )


# Achievements of two Steam games: (app id, how many, how many unlocked).
ACHIEVEMENT_GAMES = [(900001, 51, 23), (900007, 12, 12)]

ACHIEVEMENT_WORDS = [
    ("First Light", "Reach the lighthouse."),
    ("Deep Water", "Dive below 100 m."),
    ("Salt and Iron", "Repair the old pier."),
    ("Night Swimmer", "Swim across the bay after dark."),
    ("Tide Reader", "Predict ten tides in a row."),
    ("Driftwood", "Build a raft from what the sea brings."),
    ("Gull Friend", "Feed the gulls twenty times."),
    ("Storm Chaser", "Sail through a storm without damage."),
    ("Cartographer", "Map every island of the archipelago."),
    ("Shell Collector", "Find every shell on the north beach."),
    ("Keeper", "Light the lamp every night for a week."),
    ("Long Way Home", "Walk back from the far cape."),
]


def achievement_icon(path, colours, glyph, locked):
    """A 64x64 icon: a gradient and a letter, grey and dark when locked."""
    top, bottom = colours
    extra = ["-colorspace", "Gray", "-brightness-contrast", "-35x-20"] if locked else []
    subprocess.run(
        [
            "convert", "-size", "64x64", f"gradient:{top}-{bottom}",
            "-font", "DejaVu-Sans-Bold", "-fill", "white", "-pointsize", "30",
            "-gravity", "center", "-annotate", "+0+0", glyph,
            *extra, "-quality", "90", path,
        ],
        check=True,
    )


def achievements(appdata, now):
    """achievements-fixture.json, as the shell caches Steam's answers, and
    the icons, named as the shell names downloaded ones."""
    icons = os.path.join(appdata, "achievement-icons-fixture")
    os.makedirs(icons)
    palette = [("#1e3a8a", "#f97316"), ("#581c87", "#06b6d4"), ("#052e16", "#84cc16"),
               ("#7f1d1d", "#facc15"), ("#134e4a", "#5eead4"), ("#4a044e", "#f0abfc")]
    games = {}
    for appid, count, unlocked in ACHIEVEMENT_GAMES:
        items = []
        for i in range(count):
            name, description = ACHIEVEMENT_WORDS[i % len(ACHIEVEMENT_WORDS)]
            if i >= len(ACHIEVEMENT_WORDS):
                name = f"{name} {i // len(ACHIEVEMENT_WORDS) + 1}"
            urls = [f"https://example.invalid/ach/{appid}/{i}.jpg",
                    f"https://example.invalid/ach/{appid}/{i}g.jpg"]
            for url, locked in zip(urls, (False, True)):
                file = hashlib.sha256(url.encode()).hexdigest()[:32] + ".jpg"
                achievement_icon(os.path.join(icons, file), palette[i % len(palette)],
                                 name[0], locked)
            hidden = i == count - 1 and unlocked < count
            items.append({
                "id": f"ACH_{i}", "name": name,
                "description": "" if hidden else description, "hidden": hidden,
                "icon": urls[0], "icon_locked": urls[1],
                # The latest first, a few hours apart.
                "unlocked": now - 3600 * (5 + 17 * i) if i < unlocked else None,
                "percent": round(max(0.4, 92.0 - i * 1.9 - (i % 3) * 3.3), 1),
            })
        games[f"steam:{appid}"] = {"at": now, "language": "english", "list": items}
    with open(os.path.join(appdata, "achievements-fixture.json"), "w") as f:
        json.dump({"version": 1, "games": games}, f, indent=1)


def lnk(target, workdir=""):
    """A Windows shortcut (.lnk) to `target`: link info with an ANSI and a
    Unicode path, then the working folder."""
    flags = 0x01 | 0x02 | 0x80 | (0x10 if workdir else 0)
    clsid = bytes([0x01, 0x14, 0x02, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46])
    header = struct.pack("<I16sI", 0x4C, clsid, flags).ljust(0x4C, b"\0")
    id_list = struct.pack("<H", 2) + b"\0\0"
    volume = struct.pack("<IIII", 0x10, 3, 0, 0x10)
    ansi = target.encode("ascii", "replace") + b"\0"
    wide = target.encode("utf-16-le") + b"\0\0"
    base_at = 0x24 + len(volume)
    suffix_at = base_at + len(ansi)
    wide_at = suffix_at + 1
    wide_suffix_at = wide_at + len(wide)
    info = struct.pack("<9I", wide_suffix_at + 2, 0x24, 1, 0x24, base_at, 0, suffix_at,
                       wide_at, wide_suffix_at) + volume + ansi + b"\0" + wide + b"\0\0"
    strings = b""
    if workdir:
        strings = struct.pack("<H", len(workdir)) + workdir.encode("utf-16-le")
    return header + id_list + info + strings + b"\0\0\0\0"


def publisher_id(publisher):
    """The 13-character publisher id of a package family name."""
    digest = hashlib.sha256(publisher.encode("utf-16-le")).digest()[:8]
    bits = int.from_bytes(digest, "big") << 1
    alphabet = "0123456789abcdefghjkmnpqrstvwxyz"
    return "".join(alphabet[(bits >> (5 * (12 - i))) & 31] for i in range(13))


def vdf(obj, indent=0):
    tabs = "\t" * indent
    out = ""
    for key, value in obj.items():
        if isinstance(value, dict):
            out += f'{tabs}"{key}"\n{tabs}{{\n{vdf(value, indent + 1)}{tabs}}}\n'
        else:
            out += f'{tabs}"{key}"\t\t"{value}"\n'
    return out


def main(out):
    library = os.path.join(out, "library")
    covers = os.path.join(out, "appdata", "Canape", "covers-fixture")
    shutil.rmtree(out, ignore_errors=True)
    steam = os.path.join(library, "steam")
    for d in ("steamapps", "appcache/librarycache", "userdata/1/config"):
        os.makedirs(os.path.join(steam, d))
    for d in ("epic", "xbox"):
        os.makedirs(os.path.join(library, d))
    os.makedirs(covers)

    now = int(time.time())
    # Epic and GOG keep no play times: those come from Canapé's own launches.
    apps, played, gog, launched = {}, {}, [], {}
    for n, (store, title, top, bottom, days) in enumerate(GAMES):
        last = None if days is None else now - days * DAY - 3600 * (n + 1)
        slug = "".join(c for c in title if c.isalnum())
        if store == "steam":
            appid = 900001 + n
            apps[str(appid)] = "1000"
            manifest = {"AppState": {"appid": appid, "name": title, "StateFlags": 4,
                                     "installdir": title, "LastPlayed": last or 0,
                                     "SizeOnDisk": str((4 + 7 * n % 50) * 1_000_000_000)}}
            with open(os.path.join(steam, "steamapps", f"appmanifest_{appid}.acf"), "w") as f:
                f.write(vdf(manifest))
            if last:
                played[str(appid)] = {"LastPlayed": last}
            cover(os.path.join(steam, "appcache/librarycache", f"{appid}_library_600x900.jpg"),
                  title, top, bottom)
            # The wide art behind the game page.
            cover(os.path.join(steam, "appcache/librarycache", f"{appid}_library_hero.jpg"),
                  title, bottom, top, "1920x620", titled=False)
        elif store == "epic":
            item = {
                "DisplayName": title, "AppName": slug, "MainGameAppName": slug,
                "CatalogNamespace": slug.lower(), "CatalogItemId": f"{n:032x}",
                "InstallLocation": f"C:\\Program Files\\Epic Games\\{slug}",
                "LaunchExecutable": f"{slug}.exe", "bIsIncompleteInstall": False,
                "AppCategories": ["public", "games", "applications"],
            }
            if last:
                launched[f"epic:{slug}"] = last
            with open(os.path.join(library, "epic", f"{slug}.item"), "w") as f:
                json.dump(item, f, indent=1)
            cover(os.path.join(covers, f"epic_{slug}.jpg"), title, top, bottom)
        elif store == "gog":
            key = str(1400000000 + n)
            if last:
                launched[f"gog:{key}"] = last
            gog.append({"key": key, "values": {
                "gameName": title, "exe": f"C:\\GOG Games\\{title}\\{slug}.exe",
                "path": f"C:\\GOG Games\\{title}"}})
            cover(os.path.join(covers, f"gog_{key}.jpg"), title, top, bottom)
        else:
            content = os.path.join(library, "xbox", title, "Content")
            if last:
                launched[f"xbox:Fake.{slug}_{publisher_id('CN=Fake')}"] = last
            os.makedirs(os.path.join(content, "GraphicsLogo"))
            with open(os.path.join(content, "MicrosoftGame.config"), "w") as f:
                f.write(
                    '<?xml version="1.0" encoding="utf-8"?>\n<Game configVersion="1">\n'
                    f'  <Identity Name="Fake.{slug}" Publisher="CN=Fake" Version="1.0.0.0"/>\n'
                    f'  <ExecutableList><Executable Name="{slug}.exe" Id="Game"/></ExecutableList>\n'
                    f'  <ShellVisuals DefaultDisplayName="{title}" '
                    'Square480x480Logo="GraphicsLogo\\Square480x480Logo.png"/>\n</Game>\n')
            # Xbox games ship square logos: the shell shows them on a blurred copy.
            cover(os.path.join(content, "GraphicsLogo", "Square480x480Logo.png"),
                  title, top, bottom, "480x480")

    folders = {"libraryfolders": {"0": {"path": "C:\\Program Files (x86)\\Steam", "apps": apps}}}
    with open(os.path.join(steam, "steamapps", "libraryfolders.vdf"), "w") as f:
        f.write(vdf(folders))
    config = {"UserLocalConfigStore": {"Software": {"Valve": {"Steam": {"apps": played}}}}}
    with open(os.path.join(steam, "userdata/1/config/localconfig.vdf"), "w") as f:
        f.write(vdf(config))
    with open(os.path.join(library, "gog.json"), "w") as f:
        json.dump(gog, f, indent=1)
    # The "to install" list, as the shell caches it, with downloaded covers.
    owned = []
    for store, title, top, bottom, store_id in OWNED:
        game_id = f"{store}:{store_id}"
        install = ({"store": "steam", "appid": store_id} if store == "steam"
                   else {"store": "epic", "app": store_id} if store == "epic"
                   else {"store": "gog", "id": store_id})
        owned.append({"id": game_id, "title": title, "source": store,
                      "launch": {"kind": "install", "install": install},
                      "size": OWNED_SIZES.get(title)})
        cover(os.path.join(covers, game_id.replace(":", "_") + ".jpg"), title, top, bottom)
    with open(os.path.join(library, "owned.json"), "w") as f:
        json.dump({"version": 1, "games": owned}, f, indent=1)
    # Games no store knows, and shortcuts to add more.
    external = []
    os.makedirs(os.path.join(library, "covers"))
    for title, exe, colours in EXTERNAL:
        entry = {"title": title, "exe": exe}
        if colours:
            name = "".join(c for c in title if c.isalnum()).lower() + ".jpg"
            cover(os.path.join(library, "covers", name), title, *colours)
            entry["cover"] = "covers/" + name
        external.append(entry)
    with open(os.path.join(library, "games.json"), "w") as f:
        json.dump({"games": external}, f, indent=2)
    shortcuts = os.path.join(library, "shortcuts", "Games")
    os.makedirs(shortcuts)
    for name, target in SHORTCUTS:
        with open(os.path.join(shortcuts, name + ".lnk"), "wb") as f:
            f.write(lnk(target, target.rsplit("\\", 1)[0]))
    with open(os.path.join(out, "appdata", "Canape", "recent-fixture.json"), "w") as f:
        # The first game's executable, as learnt at its launch: its profile
        # shows on the game page.
        json.dump({"launched": launched, "exes": {"steam:900001": "ashentide.exe"}}, f, indent=1)
    # Time to beat, as the shell caches IGDB's answers: three times for
    # the first game, only some for the next two.
    hour = 3600
    beats = {
        "steam:900001": {"hastily": 11 * hour, "normally": 16 * hour, "completely": 38 * hour},
        "steam:900002": {"hastily": 5 * hour, "normally": 7 * hour},
        "steam:900005": {"normally": 45 * 60},
    }
    with open(os.path.join(out, "appdata", "Canape", "igdb-fixture.json"), "w") as f:
        json.dump({"version": 1, "games": {
            game_id: {"at": now, "times": dict(times, count=12)}
            for game_id, times in beats.items()}}, f, indent=1)
    achievements(os.path.join(out, "appdata", "Canape"), now)
    print(f"LOCALAPPDATA={out}/appdata CANAPE_LIBRARY_FIXTURE={library}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(os.path.abspath(sys.argv[1]))
