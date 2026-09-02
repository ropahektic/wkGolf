wKGolf.dll is a Wormkit module that allows players to play Golf inside of WA

wkGolf comes with a .png map (golf course), a golf.toml that dictates game and map configuration (tee spawn, hole, par etc) and a WA scheme file.

The scheme includes important features: full phasing of worms, damage and proyectiles and anti-lock power to make full power challenging and a game of skill.

wKGolf has certain features to try and make it as close as possible to popular golf games in the market:

Done:

Realistic Golf wind - fully revamped wind to affect grenades in a realistic way

Spin ball physics - Add Left or Right spin to your ball bounces (J for left spin, K for no spin, L for right spin) Each spin direction has three strengths, +,++ and +++. Spin is influenced by power and ball momentum.

Full power landing marker - an arrow shows the theoretical landing of your ball at full power in real time without accounting for wind

Autoteleporting - this module is totally hands off, the worms are teleported to their ball and the next tee upon course completion

HUD - the game offers a real time HUD overlay stating par, shot # and other information

Todo:

Realistic sprite spin animations - the grenade should spin accordingly to its applied spin ignoring the simple WA stock animation physics (faster roll the faster it goes).


EDITOR

wkGolf comes with a web editor so you can quickly create maps. Please ensure maps are WA valid after creation (palette limit, /8 height and width etc)


The editor can be used in: https://ropersheaven.com/web/golf-editor/


INSTALLATION

Place wkGolf.dll and golf.toml in WA's installation folder next to WA.exe

Place the map in User/SavedLevels

Place the scheme in User/Schemes


api-ms-win-core-synch-l1-2-0.dll and brcryptprimitives.dll are extra Wine dependencies to ensure the module works in ios/Linux, place in WA installation folder next to WA.exe
