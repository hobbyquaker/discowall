# OpenWrt package for discowall. Untested on a real build host as of 2026-10-10: written from the
# packages feed's Rust conventions (lang/rust, `rust-package.mk`, OpenWrt 23.05+). Copy this directory
# into a feed (e.g. packages/net/discowall), `make menuconfig` → Network → discowall.
include $(TOPDIR)/rules.mk

PKG_NAME:=discowall
PKG_VERSION:=0.1.0
PKG_RELEASE:=1

PKG_SOURCE:=$(PKG_NAME)-$(PKG_VERSION).tar.gz
PKG_SOURCE_URL:=https://codeload.github.com/hobbyquaker/discowall/tar.gz/v$(PKG_VERSION)?
PKG_HASH:=skip

PKG_MAINTAINER:=Sebastian Raff <hobbyquaker@gmail.com>
PKG_LICENSE:=MIT
PKG_LICENSE_FILES:=LICENSE

PKG_BUILD_DEPENDS:=rust/host
PKG_BUILD_PARALLEL:=1

include $(INCLUDE_DIR)/package.mk
include $(TOPDIR)/feeds/packages/lang/rust/rust-package.mk

define Package/discowall
  SECTION:=net
  CATEGORY:=Network
  TITLE:=discovery firewall: mDNS between interfaces under rules
  URL:=https://github.com/hobbyquaker/discowall
  DEPENDS:=$(RUST_ARCH_DEPENDS)
endef

define Package/discowall/description
  Forwards mDNS (Bonjour, Zeroconf) between the router's interfaces like avahi's
  reflector, but under rules: which hosts and services from which network are
  visible in which other network. discoctl edits the rules and watches the traffic.
endef

define Package/discowall/conffiles
/etc/discowall/discowall.toml
/etc/discowall/rules.toml
endef

define Package/discowall/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) $(PKG_INSTALL_DIR)/bin/discowalld $(1)/usr/bin/
	$(INSTALL_BIN) $(PKG_INSTALL_DIR)/bin/discoctl $(1)/usr/bin/
	$(INSTALL_DIR) $(1)/etc/discowall
	$(INSTALL_CONF) $(PKG_BUILD_DIR)/contrib/discowall.toml $(1)/etc/discowall/
	$(INSTALL_CONF) $(PKG_BUILD_DIR)/contrib/rules.toml $(1)/etc/discowall/
	$(INSTALL_DIR) $(1)/etc/init.d
	$(INSTALL_BIN) ./files/discowall.init $(1)/etc/init.d/discowall
endef

$(eval $(call RustBinPackage,discowall))
$(eval $(call BuildPackage,discowall))
