# OpenWrt package for discowall, built with the OpenWrt SDK and the packages feed's Rust
# (lang/rust, rust-package.mk). Target: the latest stable OpenWrt (D-12). Copy this directory into
# the SDK's package/ (or a feed), `make package/discowall/compile`.
#
# The settings live in UCI (/etc/config/discowall, D-10), created on first install by uci-defaults
# and rendered to /etc/discowall/discowall.toml by the init script; the rule chain stays in
# /etc/discowall/rules.toml (a conffile).
include $(TOPDIR)/rules.mk

PKG_NAME:=discowall
PKG_VERSION:=0.0.1
PKG_RELEASE:=1

PKG_SOURCE:=$(PKG_NAME)-$(PKG_VERSION).tar.gz
PKG_SOURCE_URL:=https://codeload.github.com/hobbyquaker/discowall/tar.gz/v$(PKG_VERSION)?
# Set to the release tarball's sha256 when the package goes to a feed.
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
  TITLE:=discovery firewall: mDNS and SSDP between interfaces under rules
  URL:=https://github.com/hobbyquaker/discowall
  DEPENDS:=$(RUST_ARCH_DEPENDS)
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

# The source is a Cargo workspace: cargo install takes one package path at a time.
define Build/Compile
	$(call Build/Compile/Cargo,crates/discowalld)
	$(call Build/Compile/Cargo,crates/discoctl)
endef

define Package/discowall/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) $(PKG_INSTALL_DIR)/bin/discowalld $(1)/usr/bin/
	$(INSTALL_BIN) $(PKG_INSTALL_DIR)/bin/discoctl $(1)/usr/bin/
	$(INSTALL_DIR) $(1)/etc/discowall
	$(INSTALL_CONF) $(PKG_BUILD_DIR)/contrib/rules.toml $(1)/etc/discowall/
	$(INSTALL_DIR) $(1)/etc/init.d
	$(INSTALL_BIN) ./files/discowall.init $(1)/etc/init.d/discowall
	$(INSTALL_DIR) $(1)/etc/uci-defaults
	$(INSTALL_BIN) ./files/discowall.uci-defaults $(1)/etc/uci-defaults/90_discowall
endef

$(eval $(call BuildPackage,discowall))
