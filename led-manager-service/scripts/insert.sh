#!/usr/bin/env bash

set -exEuo pipefail

dbus-send --print-reply --dest=io.edgehog.LedManager \
    /io/edgehog/LedManager io.edgehog.LedManager1.Insert \
    string:gpio1
