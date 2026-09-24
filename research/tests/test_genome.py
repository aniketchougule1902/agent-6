import pytest

from agent6_research.genome import StrategyGenome, assert_unique_genomes, generate_challengers


def parent() -> StrategyGenome:
    return StrategyGenome(
        model_family="lightgbm",
        calibrator="platt",
        no_trade_threshold=0.64,
        learning_rate=0.05,
        max_depth=6,
        n_estimators=500,
        min_samples_leaf=40,
        feature_schema_version="features-v1",
    )


def test_generation_is_deterministic_bounded_unique_and_parent_linked():
    a = generate_challengers(parent(), seed=42, count=16)
    b = generate_challengers(parent(), seed=42, count=16)
    assert a == b
    assert_unique_genomes(a)
    assert [x.genome_id for x in a] == [x.genome_id for x in b]
    for candidate in a:
        candidate.validate()
        assert candidate.parent_id == parent().genome_id
        assert candidate.feature_schema_version == parent().feature_schema_version
        assert candidate.genome_id != parent().genome_id


def test_seed_changes_candidate_population_without_metric_feedback():
    first = {x.genome_id for x in generate_challengers(parent(), seed=1, count=8)}
    second = {x.genome_id for x in generate_challengers(parent(), seed=2, count=8)}
    assert first != second


def test_genome_id_is_content_addressed():
    p = parent()
    assert p.genome_id == parent().genome_id
    changed = StrategyGenome(**{**p.__dict__, "no_trade_threshold": 0.66})
    assert changed.genome_id != p.genome_id


@pytest.mark.parametrize(
    "field,value",
    [
        ("model_family", "online_rl"),
        ("calibrator", "magic"),
        ("no_trade_threshold", 0.1),
        ("learning_rate", 1.0),
        ("max_depth", 99),
        ("n_estimators", 10),
        ("min_samples_leaf", 0),
        ("feature_schema_version", ""),
    ],
)
def test_invalid_or_unbounded_genomes_fail_closed(field, value):
    values = parent().__dict__.copy()
    values[field] = value
    with pytest.raises(ValueError):
        StrategyGenome(**values).validate()


def test_generation_count_is_bounded():
    with pytest.raises(ValueError):
        generate_challengers(parent(), seed=1, count=0)
    with pytest.raises(ValueError):
        generate_challengers(parent(), seed=1, count=65)


def test_duplicate_audit_rejects_same_candidate_twice():
    candidate = generate_challengers(parent(), seed=9, count=1)[0]
    with pytest.raises(ValueError, match="duplicate"):
        assert_unique_genomes([candidate, candidate])
