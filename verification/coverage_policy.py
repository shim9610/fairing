"""Reject missing modules and line/function regressions against reviewed floors."""
def validate(files, floors, required=()):
    if not floors:
        raise ValueError('Empty API contract coverage policy')
    missing = set(required) - floors.keys()
    if missing:
        raise ValueError(f'Unreviewed mediation coverage modules: {sorted(missing)}')
    for path, metrics in floors.items():
        if path not in files:
            raise ValueError(f'Missing production coverage: {path}')
        if set(metrics) != {'lines', 'functions'}:
            raise ValueError(f'Expected line and function coverage floors: {path}')
        for metric, floor in metrics.items():
            if not 0 < floor <= 100:
                raise ValueError(f'Invalid coverage floor: {path}/{metric}')
            result = files[path][metric]
            count, covered = result['count'], result['covered']
            # Recompute percentages from counts; do not trust the report's percent field.
            if count <= 0 or not 0 <= covered <= count or covered * 100 < count * floor:
                raise ValueError(f'Coverage below {floor}%: {path}/{metric} ({covered}/{count})')
