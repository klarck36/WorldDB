import unittest

from check_crate_graph import validate_graph


VALID_GRAPH = {
    "worlddb-core": set(),
    "worlddb-storage-file": {"worlddb-core"},
    "worlddb-cli": {"worlddb-core", "worlddb-storage-file"},
    "worlddb-testkit": {"worlddb-core"},
    "xtask": set(),
}


class CrateGraphPolicyTests(unittest.TestCase):
    def test_initial_workspace_graph_is_allowed(self):
        self.assertEqual(validate_graph(VALID_GRAPH), [])

    def test_core_cannot_depend_on_file_storage(self):
        graph = {crate: set(dependencies) for crate, dependencies in VALID_GRAPH.items()}
        graph["worlddb-core"].add("worlddb-storage-file")
        self.assertTrue(
            any("worlddb-core has forbidden workspace dependencies" in error
                for error in validate_graph(graph))
        )

    def test_unreviewed_external_dependency_is_rejected(self):
        self.assertTrue(
            any("unreviewed external dependencies" in error
                for error in validate_graph(VALID_GRAPH, {"worlddb-core": {"anyhow"}}))
        )

    def test_unapproved_crate_extraction_is_rejected(self):
        graph = {crate: set(dependencies) for crate, dependencies in VALID_GRAPH.items()}
        graph["worlddb-resolution"] = set()
        self.assertTrue(
            any("unexpected=['worlddb-resolution']" in error for error in validate_graph(graph))
        )


if __name__ == "__main__":
    unittest.main()
