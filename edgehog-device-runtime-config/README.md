<!--
This file is part of Edgehog.

Copyright 2025 Seco Mind Srl
Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

SPDX-License-Identifier: Apache-2.0
-->

# Edgehog Device Runtime Config

Library to read the configuration file of the device runtime

## Architecture

The configuration will be in TOML, versioned with the `version = 'v1'` field. It can be read from
multiple places and merged together.

Each version must be backwards compatible to the previous one, though breaking changes will override
the previous configuration with a new default.

All the configuration options are always active (not locked behind feature). But will warn at
runtime if the features is not enabled.
