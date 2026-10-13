# LuCI app for discowall (D-12, D-13): status, the rule chain (edited live), hosts and services,
# and the UCI settings of the discowall package. Client-side JavaScript views and a ucode rpcd
# plugin that runs `discoctl --json`. Built with the OpenWrt SDK beside contrib/openwrt/discowall:
# copy both into the SDK's package/, `./scripts/feeds install luci-base`, then
# `make package/luci-app-discowall/compile`.
include $(TOPDIR)/rules.mk

LUCI_TITLE:=LuCI support for discowall (mDNS/SSDP discovery firewall)
LUCI_DEPENDS:=+luci-base +discowall
LUCI_PKGARCH:=all

PKG_VERSION:=0.0.1
PKG_RELEASE:=1
PKG_LICENSE:=MIT
PKG_MAINTAINER:=Sebastian Raff <hobbyquaker@gmail.com>

# Outside the luci feed; inside it (applications/luci-app-discowall) this is ../../luci.mk.
include $(TOPDIR)/feeds/luci/luci.mk

# call BuildPackage - OpenWrt buildroot signature
