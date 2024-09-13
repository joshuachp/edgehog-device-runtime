#!/usr/bin/env bash

set -exEuo pipefail

dbus-send --print-reply --dest=io.edgehog.LedManager \
    /io/edgehog/LedManager io.edgehog.LedManager1.Set \
    string:gpio1 boolean:true
