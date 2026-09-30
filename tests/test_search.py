"""Search filters, ordering and retained conflicting revisions."""
import unittest

from app.search import search
from app.vault import Problem


def node(identifier, **fields):
    return {"id": identifier, "revisionId": identifier + "-v1", "title": "Memory", "body": "", "tags": [],
            "type": "memory", "kind": "record", "facts": [], "references": [], "status": "active",
            "updatedAt": "2026-09-01T12:00:00+00:00", **fields}


class SearchTests(unittest.TestCase):
    def graph(self, nodes, conflicts=None):
        return {"vaultId": "test", "nodes": nodes, "conflicts": conflicts or []}

    def test_combined_tags_categories_and_date_boundaries(self):
        a = node("a", tags=["Travel", "Budget"], kind="decision", updatedAt="2026-09-01T14:00:00+02:00")
        b = node("b", tags=["Travel"], kind="decision")
        c = node("c", tags=["Travel", "Budget"], kind="preference")
        graph = self.graph([a, b, c])
        filters = {"tag": ["travel", "BUDGET"], "kind": ["decision"], "updatedSince": ["2026-09-01T12:00:00Z"]}
        self.assertEqual(search(graph, filters)["nodes"], [a])
        self.assertEqual(search(graph, {**filters, "updatedBefore": ["2026-09-01T12:00:01Z"]})["nodes"], [a])
        self.assertEqual(search(graph, {"updatedBefore": ["2026-09-01T12:00:00Z"]})["nodes"], [])

    def test_relevance_pagination_archive_and_exact_fact_search(self):
        title = node("a", title="Budget")
        body = node("b", body="Discuss the budget", updatedAt="2026-09-20T12:00:00Z")
        archived = node("c", title="Budget", status="archived")
        fact = node("d", facts=[{"label": "Cost", "value": {"type": "money", "amount": "1200.50", "currency": "SEK"}}])
        graph = self.graph([body, archived, title, fact])
        first = search(graph, {"q": ["budget"], "limit": ["1"]})
        self.assertEqual(first["nodes"], [title])
        self.assertEqual(first["nextOffset"], 1)
        second = search(graph, {"q": ["budget"], "limit": ["1"], "offset": ["1"]})
        self.assertEqual(second["nodes"], [body])
        self.assertIsNone(second["nextOffset"])
        self.assertEqual(search(graph, {"q": ["1200.50"]})["nodes"], [fact])
        self.assertEqual(len(search(graph, {"q": ["budget"], "includeArchived": ["true"]})["nodes"]), 3)
        self.assertEqual(search(graph, {"q": ["budget"], "sort": ["updated"]})["nodes"][0], body)

    def test_conflicting_version_remains_discoverable(self):
        a = node("a", title="Old heading")
        changed = node("a", title="New plan", revisionId="a-v2", updatedAt="2026-09-30T12:00:00Z")
        conflict = {"nodeId": "a", "revisionIds": ["a-v1", "a-v2"], "versions": [a, changed]}
        result = search(self.graph([a], [conflict]), {"q": ["new plan"], "updatedSince": ["2026-09-25T00:00:00Z"]})
        self.assertEqual(result["nodes"], [a])
        self.assertEqual(result["conflicts"], [conflict])

    def test_malformed_filters_do_not_silently_broaden_search(self):
        for query in ({"updatedSince": ["2026-09-01"]}, {"includeArchived": ["yes"]}, {"kind": ["other"]},
                      {"sort": ["unknown"]}, {"q": ["a", "b"]}, {"limit": ["0"]}, {"offset": ["-1"]}, {"tags": ["typo"]}):
            with self.subTest(query=query), self.assertRaises(Problem):
                search(self.graph([]), query)


if __name__ == "__main__":
    unittest.main()
