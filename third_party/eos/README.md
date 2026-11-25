# Epic Online Services SDK runtime

`EOSSDK-Win64-Shipping.dll` is the Windows x64 runtime library of the Epic Online Services SDK, version 1.19.2.1
(SHA-256 `ab25af350c48856633d671a16e8469277f9acee39186d69fe5b2a26100d738e8`). It is Epic Games' material, kept here
with Epic's permission so the release can be packed from the repo. It is not part of MVS Classic's source, and the
MIT licence in the repo's `LICENSE` does not cover it: Epic Games licenses it under the Epic Online Services
Developer Agreement, and it is distributed only as part of MVS Classic.

`mvs-dist build --eos` packs it into the game's folder (`MVS_EOS_DLL` names another copy), with Epic's notice
(`crates/mvs-dist/notices/eos-notice.txt`) after the licence in the release's `LICENSE.txt`.

Epic, Epic Games and Epic Online Services are trademarks of Epic Games, Inc. MVS Classic is not made, endorsed or
supported by Epic Games, Inc.
