# Epic Online Services SDK runtimes

The runtime libraries of the Epic Online Services SDK, version 1.19.2.1:

| File | Platform | SHA-256 |
|---|---|---|
| `EOSSDK-Win64-Shipping.dll` | Windows x64 | `ab25af350c48856633d671a16e8469277f9acee39186d69fe5b2a26100d738e8` |
| `libEOSSDK-Linux-Shipping.so` | Linux x64 | `05c88b7adb10bfe3dc89d133af7318422838ab6909c06dcd3e74bd2af4ac2309` |

They are Epic Games' material, kept here with Epic's permission so a release can be packed from the repo. They are
not part of MVS Classic's source, and the MIT licence in the repo's `LICENSE` does not cover them: Epic Games
licenses them under the Epic Online Services Developer Agreement, and they are distributed only as part of MVS
Classic.

`mvs-dist build --eos` packs the Windows library into the game's folder (`MVS_EOS_DLL` names another copy), with
Epic's notice (`crates/mvs-dist/notices/eos-notice.txt`) after the licence in the release's `LICENSE.txt`.

Epic, Epic Games and Epic Online Services are trademarks of Epic Games, Inc. MVS Classic is not made, endorsed or
supported by Epic Games, Inc.
