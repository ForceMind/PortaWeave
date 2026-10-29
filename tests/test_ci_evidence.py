import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('ci_evidence', Path(__file__).parents[1] / 'tools/ci_evidence.py')
ci = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci)
SHA = 'a' * 40


def fixture():
    return {'source_commit': SHA, 'workflows': [{'run': {'id': 1, 'workflow_id': 2, 'head_sha': SHA, 'event': 'push', 'head_branch': 'main', 'status': 'completed', 'conclusion': 'success'}, 'jobs': [{'id': 3, 'run_id': 1, 'head_sha': SHA, 'status': 'completed', 'conclusion': 'success', 'steps': [{'status': 'completed', 'conclusion': 'success'}, {'status': 'completed', 'conclusion': 'skipped'}]}]}]}


class EvidenceTests(unittest.TestCase):
    def check(self, data):
        return ci.audit(data, SHA, 1, 1)

    def test_success_and_conditional_step_skip_are_not_product_certification(self):
        result = self.check(fixture())
        self.assertEqual(result['status'], 'PASS')
        self.assertFalse(result['product_certified'])

    def test_non_success_jobs_never_pass(self):
        for status, conclusion in [('queued', None), ('in_progress', None), ('completed', 'cancelled'), ('completed', 'skipped'), ('completed', 'failure')]:
            data = fixture()
            data['workflows'][0]['jobs'][0].update(status=status, conclusion=conclusion)
            self.assertEqual(self.check(data)['status'], 'FAIL')

    def test_success_run_cannot_hide_missing_or_failed_steps(self):
        for steps in [[], None, [{'status': 'completed', 'conclusion': 'failure'}], [{'status': 'in_progress', 'conclusion': None}]]:
            data = fixture()
            data['workflows'][0]['jobs'][0]['steps'] = steps
            self.assertEqual(self.check(data)['status'], 'FAIL')

    def test_source_event_branch_and_run_association_are_checked(self):
        for target, key, value in [('run', 'head_sha', 'b'*40), ('run', 'event', 'pull_request'), ('run', 'head_branch', 'other'), ('job', 'head_sha', 'b'*40), ('job', 'run_id', 2)]:
            data = fixture()
            row = data['workflows'][0]['run'] if target == 'run' else data['workflows'][0]['jobs'][0]
            row[key] = value
            self.assertEqual(self.check(data)['status'], 'FAIL')

    def test_duplicates_and_incomplete_counts_fail(self):
        data = fixture()
        data['workflows'].append(copy.deepcopy(data['workflows'][0]))
        self.assertEqual(ci.audit(data, SHA, 2, 2)['status'], 'FAIL')
        self.assertEqual(ci.audit(fixture(), SHA)['status'], 'FAIL')

    def test_malformed_inputs_and_sha_rejected(self):
        for data in [None, [], {}, {'source_commit': SHA}, {'source_commit': SHA, 'workflows': [None]}]:
            self.assertEqual(self.check(data)['status'], 'FAIL')
        with self.assertRaises(ValueError):
            ci.audit(fixture(), 'main')
        with self.assertRaises(ValueError):
            ci.audit(fixture(), SHA, 0, 1)
