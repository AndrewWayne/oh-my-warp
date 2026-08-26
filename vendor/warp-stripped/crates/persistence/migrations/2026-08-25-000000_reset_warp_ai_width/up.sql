-- SPDX-License-Identifier: AGPL-3.0-only
--
-- omw-authored file in the in-tree Warp fork (warpdotdev/warp), part of the
-- AGPL-3.0 derivative work. See specs/fork-strategy.md section 3.
-- Copyright (C) 2026 Shenhao Miao and the omw contributors
-- Copyright (C) 2020-2026 Denver Technologies, Inc.
-- Licensed under the GNU Affero General Public License, version 3.

-- Apply the compact Agent panel default once to windows saved by earlier builds.
UPDATE windows
SET warp_ai_width = 360
WHERE warp_ai_width IS NOT NULL;
