-- SPDX-License-Identifier: AGPL-3.0-only
--
-- omw-authored file in the in-tree Warp fork (warpdotdev/warp), part of the
-- AGPL-3.0 derivative work. See specs/fork-strategy.md section 3.
-- Copyright (C) 2026 Shenhao Miao and the omw contributors
-- Copyright (C) 2020-2026 Denver Technologies, Inc.
-- Licensed under the GNU Affero General Public License, version 3.

-- This data migration is intentionally irreversible. Restoring the old value
-- would overwrite widths that users selected after the migration ran.
SELECT 1;
