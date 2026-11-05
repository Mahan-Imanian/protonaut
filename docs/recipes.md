# Recipes

Protonaut tests a packaged Windows build, so it does not care which engine made it. Each recipe below produces a Windows build, then runs Protonaut on `ubuntu-24.04`.

This project runs the Godot path in CI (see [real-games.md](compatibility/real-games.md)). The Unity and Unreal recipes follow the same pattern, but this project has not run them. If you use one, please [report how it went](https://github.com/Mahan-Imanian/protonaut/issues/new/choose).

## Any engine: test a build made elsewhere

Build on whatever runner your engine needs, upload the result, and test it in a second job.

```yaml
jobs:
  build:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - run: ./build-windows.ps1
      - uses: actions/upload-artifact@v4
        with:
          name: windows-build
          path: dist/Windows

  protonaut:
    needs: build
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/download-artifact@v4
        with:
          name: windows-build
          path: dist/Windows
      - uses: Mahan-Imanian/protonaut@v0
        with:
          build: dist/Windows
          exe: MyGame.exe
```

## Godot

Godot exports a Windows build from Linux, so one job is enough. Install Godot and its export templates first, and define a `Windows Desktop` export preset in your project.

```yaml
- run: |
    mkdir -p dist/Windows
    godot --headless --export-release "Windows Desktop" dist/Windows/MyGame.exe
- uses: Mahan-Imanian/protonaut@v0
  with:
    build: dist/Windows
    exe: MyGame.exe
```

## Unity

Build with [GameCI](https://game.ci), which needs your Unity license in repository secrets, then test the output in a second job as in the first recipe.

```yaml
- uses: game-ci/unity-builder@v4
  env:
    UNITY_LICENSE: ${{ secrets.UNITY_LICENSE }}
    UNITY_EMAIL: ${{ secrets.UNITY_EMAIL }}
    UNITY_PASSWORD: ${{ secrets.UNITY_PASSWORD }}
  with:
    targetPlatform: StandaloneWindows64
    buildName: MyGame
- uses: actions/upload-artifact@v4
  with:
    name: windows-build
    path: build/StandaloneWindows64
```

## Unreal Engine

Package on a Windows runner that has the engine installed, usually self-hosted, then test the output in a second job as in the first recipe.

```yaml
- run: >
    RunUAT.bat BuildCookRun -project="$PWD\MyGame.uproject"
    -platform=Win64 -clientconfig=Shipping -cook -build -stage -pak -archive
    -archivedirectory="$PWD\dist"
- uses: actions/upload-artifact@v4
  with:
    name: windows-build
    path: dist/Windows
```

Set `exe` to the executable in the top folder of the build, for example `MyGame.exe`.

## Add a status badge

Save your workflow as `.github/workflows/protonaut.yml`, then add this to your README:

```markdown
[![Protonaut](https://github.com/OWNER/REPO/actions/workflows/protonaut.yml/badge.svg)](https://github.com/OWNER/REPO/actions/workflows/protonaut.yml)
```
