# OpenWrt package for discowall from the release's prebuilt static musl binaries (task 16): the same
# package as ../discowall (name, files, conffiles, init script, uci-defaults), but instead of compiling
# with the packages feed's Rust (about an hour per architecture) it installs discowalld and discoctl
# from the release tarball discowall-<version>-<rust target>.tar.gz. No feeds are needed.
#
# Use one of the two directories, never both: copy this one as package/discowall into the SDK, with
# the init files dereferenced (`files` is a link to ../discowall/files):
#   cp -rL contrib/openwrt/discowall-prebuilt <sdk>/package/discowall
#   make defconfig && make package/discowall/compile
# The SDK downloads the tarball from the GitHub release; put it into <sdk>/dl/ to build offline
# (the release workflow does that). contrib/openwrt/build-packages.sh does all of it.
#
# The release builds x86_64, aarch64 and armv7 (hard-float, VFPv3-D16). A static binary runs on every
# CPU of its family, so this package builds for every OpenWrt x86_64, aarch64 and ARMv7 architecture
# (D-14); older ARM, MIPS and the rest are not covered (DEPENDS hides the package there). The armv7
# binary needs a VFP unit: on ARMv7 CPUs without one (arm_cortex-a9 on bcm53xx, BCM4708/4709) it is
# expected to die with SIGILL -- read from the binary's ELF attributes (Tag_FP_arch: VFPv3), not measured.
include $(TOPDIR)/rules.mk

PKG_NAME:=discowall
PKG_VERSION:=0.0.3
PKG_RELEASE:=1

ifeq ($(ARCH),x86_64)
  DISCOWALL_TARGET:=x86_64-unknown-linux-musl
else ifeq ($(ARCH),aarch64)
  DISCOWALL_TARGET:=aarch64-unknown-linux-musl
else
  DISCOWALL_TARGET:=armv7-unknown-linux-musleabihf
endif

PKG_SOURCE:=$(PKG_NAME)-$(PKG_VERSION)-$(DISCOWALL_TARGET).tar.gz
PKG_SOURCE_URL:=https://github.com/hobbyquaker/discowall/releases/download/v$(PKG_VERSION)/
# The release publishes no checksums yet; the tarball comes from the release itself over HTTPS.
PKG_HASH:=skip
PKG_BUILD_DIR:=$(BUILD_DIR)/$(PKG_NAME)-$(PKG_VERSION)-$(DISCOWALL_TARGET)

PKG_MAINTAINER:=Sebastian Raff <hobbyquaker@gmail.com>
PKG_LICENSE:=MIT
PKG_LICENSE_FILES:=LICENSE

include $(INCLUDE_DIR)/package.mk

# The release binaries go in as they are, byte for byte.
STRIP:=:
RSTRIP:=:

define Package/discowall
  SECTION:=net
  CATEGORY:=Network
  TITLE:=discovery firewall: mDNS and SSDP between interfaces under rules
  URL:=https://github.com/hobbyquaker/discowall
  DEPENDS:=@(x86_64||aarch64||arm_v7)
endef

define Package/discowall/description
  Forwards mDNS (Bonjour, Zeroconf) and SSDP (UPnP) between the router's
  interfaces like avahi's reflector, but under rules: which hosts and services
  from which network are visible in which other network. discoctl edits the
  rules and watches the traffic.
endef

define Package/discowall/conffiles
/etc/config/discowall
/etc/discowall/rules.toml
endef

define Build/Compile
endef

define Package/discowall/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) $(PKG_BUILD_DIR)/discowalld $(1)/usr/bin/
	$(INSTALL_BIN) $(PKG_BUILD_DIR)/discoctl $(1)/usr/bin/
	$(INSTALL_DIR) $(1)/etc/discowall
	$(INSTALL_CONF) $(PKG_BUILD_DIR)/rules.toml $(1)/etc/discowall/
	$(INSTALL_DIR) $(1)/etc/init.d
	$(INSTALL_BIN) ./files/discowall.init $(1)/etc/init.d/discowall
	$(INSTALL_DIR) $(1)/etc/uci-defaults
	$(INSTALL_BIN) ./files/discowall.uci-defaults $(1)/etc/uci-defaults/90_discowall
endef

$(eval $(call BuildPackage,discowall))
