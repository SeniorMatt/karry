#!/bin/bash
# Builds Karry-x86_64.AppImage (run on Arch after building target/release/karry-cli and build/app/karry).
set -euo pipefail
cd "$(dirname "$0")/../.."
A=AppDir
Q=/usr/lib/qt6/plugins
rm -rf $A && mkdir -p $A/usr/bin $A/usr/lib/karry $A/usr/lib/qt6/plugins/kf6 $A/usr/lib/qt6/plugins/platformthemes $A/usr/share/icons/hicolor/scalable/apps
cp build/app/karry $A/usr/bin/
cp target/release/karry-cli $A/usr/lib/karry/
cat > $A/usr/bin/karry-cli <<'W'
#!/bin/sh
H="$(dirname "$(readlink -f "$0")")/.."
exec "$H/lib/ld-linux-x86-64.so.2" --library-path "$H/lib" "$H/lib/karry/karry-cli" "$@"
W
chmod +x $A/usr/bin/karry-cli
cp packaging/appimage/AppRun $A/ && chmod +x $A/AppRun
cp app/org.kde.karry.desktop app/org.kde.karry.svg $A/
cp app/org.kde.karry.svg $A/usr/share/icons/hicolor/scalable/apps/
for d in platforms wayland-shell-integration wayland-decoration-client wayland-graphics-integration-client xcbglintegrations imageformats iconengines styles kiconthemes6; do
  [ -d $Q/$d ] && cp -r $Q/$d $A/usr/lib/qt6/plugins/
done
cp $Q/platformthemes/KDEPlasmaPlatformTheme6.so $A/usr/lib/qt6/plugins/platformthemes/
for p in FrameworkIntegrationPlugin.so kio kiod kwindowsystem; do cp -r $Q/kf6/$p $A/usr/lib/qt6/plugins/kf6/; done
for t in breeze breeze-dark; do cp -r /usr/share/icons/$t $A/usr/share/icons/; done
rm -f $A/usr/share/icons/*/*.rcc
mkdir -p $A/usr/share/color-schemes && cp /usr/share/color-schemes/Breeze*.colors $A/usr/share/color-schemes/
# every library the binaries and plugins need, glibc and its loader included
find $A/usr -type f \( -name '*.so*' -o -path '*/bin/karry' -o -path '*/karry/karry-cli' \) -exec ldd {} + 2>/dev/null |
  awk '$3 ~ /^\// {print $3} $1 ~ /^\/.*ld-linux/ {print $1}' | sort -u |
  grep -v -E '/lib(GL|EGL|GLX|OpenGL|GLdispatch|gbm|drm|vulkan)[.-]' | while read -r f; do cp -L "$f" $A/usr/lib/; done
cp -L /usr/lib/ld-linux-x86-64.so.2 $A/usr/lib/
T=appimagetool-1.9.1-x86_64.AppImage R=runtime-20251108-x86_64
curl -fsSL -o $T https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
curl -fsSL -o $R https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
echo "ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0  $T
2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d  $R" | sha256sum -c -
chmod +x $T
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 ./$T --no-appstream --runtime-file $R --comp zstd $A Karry-x86_64.AppImage
