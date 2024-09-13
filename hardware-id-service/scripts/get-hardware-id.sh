#!/usr/bin/env bash

set -exEuo pipefail

dbus-send --print-reply --dest=io.edgehog.Device /io/edgehog/Device io.edgehog.Device1.GetHardwareId \
    'string:f79ad91f-c638-4889-ae74-9d001a3b4cf8'
