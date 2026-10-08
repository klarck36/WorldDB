import unittest

from check_crate_graph import validate_graph


VALID_GRAPH = {
    "worlddb-core": set(),
    "worlddb-storage-file": {"worlddb-core"},
    "worlddb-process-adapter": set(),
    "worlddb-cli": {"worlddb-core", "worlddb-process-adapter", "worlddb-storage-file"},
    "worlddb-testkit": {"worlddb-core"},
    "xtask": set(),
}
VALID_EXTERNAL = {
    "worlddb-core": {"blake3", "getrandom", "uuid"},
    "worlddb-storage-file": {"blake3", "fs4", "getrandom", "libc", "windows-sys"},
    "worlddb-process-adapter": {"libc", "windows-sys"},
    "worlddb-testkit": {"rusqlite"},
    "worlddb-cli": {"blake3", "getrandom"},
}


class CrateGraphPolicyTests(unittest.TestCase):
    def test_initial_workspace_graph_is_allowed(self):
        self.assertEqual(validate_graph(VALID_GRAPH, VALID_EXTERNAL), [])

    def test_core_cannot_depend_on_file_storage(self):
        graph = {crate: set(dependencies) for crate, dependencies in VALID_GRAPH.items()}
        graph["worlddb-core"].add("worlddb-storage-file")
        self.assertTrue(
            any(
                "worlddb-core has forbidden workspace dependencies" in error
                for error in validate_graph(graph, VALID_EXTERNAL)
            )
        )

    def test_unreviewed_external_dependency_is_rejected(self):
        external = {crate: set(packages) for crate, packages in VALID_EXTERNAL.items()}
        external["worlddb-core"].add("anyhow")
        self.assertTrue(
            any(
                "unreviewed external dependencies" in error
                for error in validate_graph(VALID_GRAPH, external)
            )
        )

    def test_reviewed_external_dependency_cannot_be_removed_silently(self):
        self.assertTrue(
            any(
                "missing expected reviewed external dependencies" in error
                for error in validate_graph(VALID_GRAPH, {"worlddb-core": {"uuid"}})
            )
        )

    def test_unapproved_crate_extraction_is_rejected(self):
        graph = {crate: set(dependencies) for crate, dependencies in VALID_GRAPH.items()}
        graph["worlddb-resolution"] = set()
        self.assertTrue(
            any(
                "unexpected=['worlddb-resolution']" in error
                for error in validate_graph(graph, VALID_EXTERNAL)
            )
        )


if __name__ == "__main__":
    unittest.main()
