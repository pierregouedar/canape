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
"""

import hashlib
import json
import os
import shutil
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
                      "launch": {"kind": "install", "install": install}})
        cover(os.path.join(covers, game_id.replace(":", "_") + ".jpg"), title, top, bottom)
    with open(os.path.join(library, "owned.json"), "w") as f:
        json.dump({"version": 1, "games": owned}, f, indent=1)
    with open(os.path.join(out, "appdata", "Canape", "recent-fixture.json"), "w") as f:
        # The first game's executable, as learnt at its launch: its profile
        # shows on the game page.
        json.dump({"launched": launched, "exes": {"steam:900001": "ashentide.exe"}}, f, indent=1)
    print(f"LOCALAPPDATA={out}/appdata CANAPE_LIBRARY_FIXTURE={library}")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(os.path.abspath(sys.argv[1]))
