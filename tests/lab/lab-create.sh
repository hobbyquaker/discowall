#!/bin/sh
# Creates (or destroys) the discowall end-to-end lab on a Proxmox VE node: four unprivileged Debian LXC
# containers on one VLAN-aware bridge. Run it on the node as root. It touches only the container ids it is
# given; everything else on the node is left alone. The topology it builds is the one tests/lab/README.md
# describes; tests/lab/lab.py drives the containers afterwards.
#
#   lab-create.sh create     create and start the containers (idempotent: existing ids are skipped)
#   lab-create.sh destroy    stop and destroy the containers (only those whose hostname starts with $PREFIX)
#   lab-create.sh status     pct status of the four
#
# Parameters, as environment variables with defaults:
set -eu

TEMPLATE=${TEMPLATE:-lxc-templates:vztmpl/debian-13-standard_13.1-2_amd64.tar.zst}
STORAGE=${STORAGE:-local-zfs}
BRIDGE=${BRIDGE:-vmbr0}
PREFIX=${PREFIX:-discowall-}          # hostnames: ${PREFIX}router, ${PREFIX}lanhost, ...
ROUTER=${ROUTER:-900}
LANHOST=${LANHOST:-901}
IOTHOST=${IOTHOST:-902}
GUESTHOST=${GUESTHOST:-903}
TAG_LAN=${TAG_LAN:-110}
TAG_IOT=${TAG_IOT:-120}
TAG_GUEST=${TAG_GUEST:-130}
# The lab networks: 10.<tag>.0.0/24 and fd00:<tag>::/64; the router is .1, the hosts .20/.30/.40.
MEMORY=${MEMORY:-512}
DISK=${DISK:-4}

common="--unprivileged 1 --ostype debian --memory $MEMORY --swap 0 --cores 1 --onboot 0 --features nesting=1 --tags discowall-lab"
# No ip6=auto here: ifupdown2 then waits for a DHCPv6 answer and leaves the lab interfaces down if none comes.
mgmt="name=eth0,bridge=$BRIDGE,ip=dhcp"

net() { # net <ifname> <tag> <host-octet>
    echo "name=$1,bridge=$BRIDGE,tag=$2,ip=10.$2.0.$3/24,ip6=fd00:$2::$3/64"
}

create_one() { # create_one <id> <hostname> <net1> [<net2> <net3>]
    id=$1; host=$2; shift 2
    if pct status "$id" >/dev/null 2>&1; then
        echo "$id ($host): exists, skipped"
        return
    fi
    n=1; nets=""
    for spec in "$@"; do nets="$nets --net$n $spec"; n=$((n + 1)); done
    # shellcheck disable=SC2086
    pct create "$id" "$TEMPLATE" --hostname "$host" --rootfs "$STORAGE:$DISK" --net0 "$mgmt" $nets $common \
        --description "discowall end-to-end lab (tests/lab in github.com/hobbyquaker/discowall); safe to destroy"
    pct start "$id"
    echo "$id ($host): created and started"
}

case ${1:-} in
create)
    create_one "$ROUTER" "${PREFIX}router" "$(net lan "$TAG_LAN" 1)" "$(net iot "$TAG_IOT" 1)" "$(net guest "$TAG_GUEST" 1)"
    create_one "$LANHOST" "${PREFIX}lanhost" "$(net lan "$TAG_LAN" 20)"
    create_one "$IOTHOST" "${PREFIX}iothost" "$(net iot "$TAG_IOT" 30)"
    create_one "$GUESTHOST" "${PREFIX}guesthost" "$(net guest "$TAG_GUEST" 40)"
    ;;
destroy)
    for id in "$ROUTER" "$LANHOST" "$IOTHOST" "$GUESTHOST"; do
        pct status "$id" >/dev/null 2>&1 || { echo "$id: absent"; continue; }
        host=$(pct config "$id" | sed -n 's/^hostname: //p')
        case $host in
        "$PREFIX"*) ;;
        *) echo "$id: hostname '$host' is not ours, refusing"; exit 1 ;;
        esac
        pct stop "$id" >/dev/null 2>&1 || true
        pct destroy "$id" --purge
        echo "$id ($host): destroyed"
    done
    ;;
status)
    for id in "$ROUTER" "$LANHOST" "$IOTHOST" "$GUESTHOST"; do
        printf '%s: ' "$id"; pct status "$id" 2>&1 || true
    done
    ;;
*)
    echo "usage: $0 create|destroy|status" >&2
    exit 2
    ;;
esac
