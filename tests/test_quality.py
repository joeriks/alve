"""The mandatory preview cannot be bypassed by an AI client."""
import unittest
from unittest.mock import patch

from app.quality import QualityGate
from app.vault import Problem


CONFIRMATION = {"userConfirmed": True, "concise": True, "accurateToSource": True, "structured": True,
                "sourceBasis": "user_statement", "basis": "The user explicitly stated this preference.", "uncertainties": ""}
CONTENT = {"title": "Prefer morning meetings", "body": "Schedule meetings in the morning where possible.",
           "type": "memory", "kind": "preference"}


class QualityTests(unittest.TestCase):
    def setUp(self):
        self.gate = QualityGate()

    def prepare(self):
        return self.gate.prepare({"content": CONTENT}, "connection-a")

    def test_exact_preview_requires_user_confirmation(self):
        preview = self.prepare()
        self.assertEqual(preview["status"], "confirmation_required")
        self.assertFalse(preview["checks"]["factualTruthVerified"])
        data = {"reviewToken": preview["reviewToken"], "confirmation": CONFIRMATION}
        payload, confirmation = self.gate.confirmed(data, "connection-a")
        self.assertEqual(payload["content"], preview["content"])
        self.assertTrue(confirmation["userConfirmed"])
        for field in ("concise", "accurateToSource", "structured", "userConfirmed"):
            with self.subTest(field=field), self.assertRaises(Problem):
                self.gate.confirmed({**data, "confirmation": {**CONFIRMATION, field: False}}, "connection-a")
        with self.assertRaises(Problem):
            self.gate.confirmed({**data, "content": {**CONTENT, "title": "Changed"}}, "connection-a")

    def test_connection_expiry_lock_and_replay(self):
        preview = self.prepare()
        data = {"reviewToken": preview["reviewToken"], "confirmation": CONFIRMATION}
        with self.assertRaises(Problem):
            self.gate.confirmed(data, "connection-b")
        with patch("app.quality.time.monotonic", return_value=10**15), self.assertRaises(Problem):
            self.gate.confirmed(data, "connection-a")
        self.gate.consume(preview["reviewToken"])
        with self.assertRaises(Problem):
            self.gate.confirmed(data, "connection-a")
        self.prepare()
        self.gate.clear()
        self.assertEqual(self.gate.tickets, {})

    def test_categories_concision_exact_facts_and_reference_basis(self):
        with self.assertRaises(Problem):
            self.gate.prepare({"content": {"title": "No categories"}}, "a")
        with self.assertRaises(Problem):
            self.gate.prepare({"content": {**CONTENT, "body": "word " * 301}}, "a")
        with self.assertRaises(Problem):
            self.gate.prepare({"content": {**CONTENT, "title": "x" * 121}}, "a")
        fact = {"key": "cost", "label": "Cost", "value": {"type": "money", "amount": "1200.50", "currency": "SEK"}}
        preview = self.gate.prepare({"content": {**CONTENT, "facts": [fact]}}, "a")
        self.assertEqual(preview["content"]["facts"][0]["value"], fact["value"])
        with self.assertRaises(Problem):
            self.gate.confirmed({"reviewToken": preview["reviewToken"], "confirmation": {**CONFIRMATION, "sourceBasis": "reference"}}, "a")


if __name__ == "__main__":
    unittest.main()
