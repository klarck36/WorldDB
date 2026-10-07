from __future__ import annotations

import copy
import unittest

from check_fuzz_targets import (
    DECODER_HEADER,
    DECODER_FAMILY_ROUTES,
    FuzzInventoryError,
    RESOURCE_HEADER,
    RUNNER_HEADER,
    SEED_HEADER,
    TARGET_HEADER,
    parse_tsv,
    validate_tables,
    verify,
)


class FuzzTargetInventoryTests(unittest.TestCase):
    def load_tables(self) -> tuple[list[dict[str, str]], ...]:
        return (
            parse_tsv("policy/fuzz-targets.tsv", TARGET_HEADER),
            parse_tsv("policy/decoder-inventory.tsv", DECODER_HEADER),
            parse_tsv("policy/fuzz-runners.tsv", RUNNER_HEADER),
            parse_tsv("policy/fuzz-resource-profiles.tsv", RESOURCE_HEADER),
            parse_tsv("policy/decoder-seeds.tsv", SEED_HEADER),
        )

    def assert_tables_rejected(self, tables: tuple[list[dict[str, str]], ...], message: str) -> None:
        with self.assertRaisesRegex(FuzzInventoryError, message):
            validate_tables(*tables)

    def test_checked_in_inventory_has_seeds_limits_and_runners_for_every_target(self) -> None:
        decoder_count, entrypoint_count = verify()
        self.assertEqual(decoder_count, 92)
        self.assertEqual(entrypoint_count, 54)

    def test_every_registered_decoder_family_has_a_runner_route(self) -> None:
        self.assertEqual("rust_core_decoder", DECODER_FAMILY_ROUTES["record"])
        self.assertEqual("typescript_transport", DECODER_FAMILY_ROUTES["typescript"])

    def test_tsv_reader_rejects_an_unexpected_header(self) -> None:
        with self.assertRaises(FuzzInventoryError):
            parse_tsv("policy/decoder-inventory.tsv", ["decoder_id", "family"])

    def test_missing_parser_target_is_rejected(self) -> None:
        tables = list(self.load_tables())
        tables[0] = [row for row in tables[0] if row["target_id"] != "storage_sharing_export"]
        self.assert_tables_rejected(tables, "required parser targets are missing")

    def test_source_code_cannot_be_registered_as_raw_seed_input(self) -> None:
        tables = copy.deepcopy(self.load_tables())
        target = next(row for row in tables[0] if row["target_id"] == "cli_arguments")
        target["seed_corpus"] = "crates/worlddb-cli/src/cli.rs"
        self.assert_tables_rejected(tables, "source code cannot serve as a raw seed corpus")

    def test_seed_cannot_exceed_its_target_input_limit(self) -> None:
        tables = copy.deepcopy(self.load_tables())
        profile = next(row for row in tables[3] if row["profile_id"] == "small_text")
        profile["max_input_bytes"] = "16"
        self.assert_tables_rejected(tables, "seed exceeds max_input_bytes")

    def test_24_hour_campaign_cannot_be_shortened(self) -> None:
        tables = copy.deepcopy(self.load_tables())
        profile = next(row for row in tables[3] if row["profile_id"] == "small_text")
        profile["campaign_duration_seconds"] = "86399"
        self.assert_tables_rejected(tables, "each campaign must request 24 hours")

    def test_import_targets_cannot_be_routed_to_the_decoder_followup(self) -> None:
        tables = copy.deepcopy(self.load_tables())
        target = next(row for row in tables[0] if row["target_id"] == "storage_sharing_export")
        target["followup_task"] = "M9-04a"
        self.assert_tables_rejected(tables, "must route to M9-04b")

    def test_duplicate_target_id_is_rejected(self) -> None:
        tables = copy.deepcopy(self.load_tables())
        tables[0].append(dict(tables[0][0]))
        self.assert_tables_rejected(tables, "duplicate fuzz target")


if __name__ == "__main__":
    unittest.main()
