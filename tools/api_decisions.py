#!/usr/bin/env python3
"""Reconcile an individually indexed API decision ledger with compiler exports."""
import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LEDGER = ROOT / 'docs/api-decisions.json'
MUTABLE_LOANS = {
    'desktop_mut': 'set_wallpaper, set_desktop_page, swipe_desktop_to, set_dock_placement, set_desktop_legibility, set_desktop_slot_painter; icon declarations still use add/remove/set_badge',
    'status_bar_mut': 'status_item_spec/set_status_item_spec, set_status_items and set_status_bar_* operations',
    'nav_bar_mut': 'set_nav_style, set_nav_enabled, set_nav_height, set_nav_background; back availability is derived from workspace state',
    'metrics_spec_mut': 'metrics_spec returns an owned copy; commit through set_metrics_spec',
    'services_mut': 'set_services, set_*_backend and request_service(ServiceRequest); retain custom-backend command senders or use admitted Cx services',
    'access_mut': 'set_policy and set_subject; set_authenticator and correlated responses remain the authentication path',
    'osk_mut': 'set_osk_layout, toggle_osk_language, toggle_osk and hide_osk',
}


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()


def contracts(report):
    """Fold actual reexports into canonical identities, retaining every exported alias."""
    result = {}
    for profile, payload in report['profiles'].items():
        aliases = {}
        containers = {'struct', 'enum', 'trait', 'module', 'union'}
        for key, value in payload['items'].items():
            if value['kind'] != 'use' or value['glob']:
                continue
            exported, target = key.split('#')[0], value['target']
            kinds = {record['kind'] for name, record in payload['items'].items()
                     if name.split('#')[0] == target and record['kind'] != 'use'}
            if not kinds:
                kinds = {record['kind'] for name, record in payload['items'].items()
                         if name.split('#')[0] == exported and record['kind'] != 'use'}
            aliases[exported] = (target, kinds)

        def canonical(path, kind):
            applied = set()
            while True:
                matches = [prefix for prefix, (_, kinds) in aliases.items()
                           if (path == prefix and (kind == 'use' or kind in kinds))
                           or (path.startswith(prefix + '::') and kinds & containers)]
                if not matches:
                    return path
                prefix = max(matches, key=len)
                target = aliases[prefix][0] + path[len(prefix):]
                if target == path:
                    return path
                if prefix in applied:
                    raise ValueError('Cyclic API alias: ' + prefix)
                applied.add(prefix)
                path = target

        for exported, declaration in payload['items'].items():
            path, kind = exported.rsplit('#', 1)
            key = canonical(path, kind) + '#' + kind
            item = result.setdefault(key, {'contracts': {}, 'exports': {}})
            fingerprint = digest(declaration)
            if profile in item['contracts'] and item['contracts'][profile] != fingerprint:
                raise ValueError('Conflicting canonical API: ' + key)
            item['contracts'][profile] = fingerprint
            item['exports'].setdefault(profile, []).append(exported)
    return dict(sorted(result.items()))


def category(key, traits):
    path, kind = key.rsplit('#', 1)
    if path in traits or path.rsplit('::', 1)[0] in traits:
        return 'external-traits' if 'ProtectedDeclaration' not in path and 'HostedShell' not in path else 'sealed-registration'
    if '::config::' in path or any(name in path for name in ('::Options', '::InboxConfig', '::SettingsConfig')):
        return 'configuration'
    if '::access::' in path:
        return 'sessions-and-access'
    if '::shell::' in path:
        return 'shell-host-and-diagnostics'
    if '::screen::' in path or '::elements::' in path or path.endswith(('::screen', '::action', '::tile', '::nav_item', '::status_item')):
        return 'registration-and-elements'
    if '::services::' in path:
        return 'backend-integration'
    if '::desktop::' in path or '::chrome::' in path or '::osk::' in path:
        return 'standalone-components'
    return 'standalone-values-and-helpers'


RATIONALE = {
    'configuration': 'Construct extensible configuration with Default/constructors. Public fields remain value customization contracts; added fields need defaults. Closed rendering value models are classified separately.',
    'sessions-and-access': 'Keep issuer/controller ownership, read-only tokens, correlated authentication and per-operation enforcement. Permission names belong to applications. No caller may obtain mutable token authority from an observer.',
    'shell-host-and-diagnostics': 'Host mutations use named operations or owned values, not mutable component loans. Read-only legacy queries remain supported; protected observation uses ShellView only. Backend/recovery setup remains trusted host code.',
    'registration-and-elements': 'Retain declarative registration and application callback contracts. Protected registration uses application permission values and the common mediation contract; direct standalone/ordinary use retains its documented trust boundary.',
    'external-traits': 'Retain application implementations and each declared method signature/default. New required methods are breaking; changes to defaults require behavior, object-safety and authority review.',
    'sealed-registration': 'Retain library-only implementation and validated registration/hosting. No external implementation or ordinary-shell conversion is added.',
    'backend-integration': 'Retain backend implementation contracts and admitted Cx access. Host backend replacement attaches the waker before publication. ServiceRequest carries explicit synchronous host operations without exposing the store.',
    'standalone-components': 'Retain caller-owned component construction and manipulation. Shell no longer lends its mutable component instances. Executable declarations remain registered through the library, not by changing diagnostic state.',
    'standalone-values-and-helpers': 'Retain this standalone value/helper contract and exported aliases. Existing fields, variants, signatures and reexported upstream types remain compatibility commitments; closed models require explicit migration if extended.',
}


def replacement(key):
    path, kind = key.rsplit('#', 1)
    member = path.rsplit('::', 1)[-1]
    if '::Shell::' in path and member in MUTABLE_LOANS:
        return MUTABLE_LOANS[member]
    if '::Registry' in path:
        if member in {'add', 'remove'}:
            return 'Shell::' + member + '; protected registration uses ProtectedShell with application permission values'
        if member == 'screens':
            return 'ShellView::screen_ids; borrowing the stored declaration collection is removed'
        if member == 'screen':
            return 'ShellView::has_screen/screen_is_resident; borrowing a stored ScreenDecl is removed'
        if member == 'has_screen':
            return 'ShellView::has_screen'
        if member == 'new' or kind in {'struct', 'use'}:
            return 'Shell::builder owns registration; standalone Registry construction is intentionally removed'
        return 'No raw-store equivalent is retained; keep declared IDs in the host and use supported ShellView queries instead of borrowing implementation collections/kinds'
    if path.endswith('::Shell::registry') or path.endswith('::ProtectedShell::shell'):
        return 'Shell::observe()/ProtectedShell::observe(): ShellView queries; no registry or shell loan'
    if '::SessionGrant::' in path and kind == 'struct_field':
        return 'SessionGrant::new/base/temporary/inactive and gate/is_base/temporary_until accessors'
    raise ValueError('Removal has no reviewed migration: ' + key)


def change_details(key, decision):
    """Concrete decisions for every intentional removal/signature change."""
    path, kind = key.rsplit('#', 1)
    if decision == 'remove':
        if '::SessionGrant::' in path:
            return {
                'current_usage': 'Public grant field access is removed. Construct the complete grant with SessionGrant::new; read gate(), is_base() and temporary_until().',
                'reason': 'Keep the grant representation private so adding internal state does not require changing downstream literals; construction and later observation use explicit methods.',
                'replacement': replacement(key),
                'compatibility': 'External grant literals and field accesses require the documented constructor/getter migration.',
            }
        if '::Registry' in path:
            return {
                'current_usage': 'Register through the host and observe supported queries; public Registry construction and collection/declaration loans are removed.',
                'reason': 'Registry is an implementation store. Keeping its standalone constructor and collection APIs public would preserve coupling even after removing Shell::registry.',
                'replacement': replacement(key),
                'compatibility': 'Source migration is required. Registration operations have host replacements; raw collection/kind/definition inspection has no equivalent retained store API.',
            }
        return {
            'current_usage': 'The previous component/store loan is removed; use the named replacement or observer queries.',
            'reason': 'Stop coupling the host to mutable implementation stores. The library owns derived-state invalidation and protected hosts never regain an ordinary-shell loan.',
            'replacement': replacement(key),
            'compatibility': 'Calls to this removed API require source migration; standalone caller-owned components remain supported.',
        }
    if decision != 'change':
        return {}
    if kind == 'struct' and ('::config::' in path or path.endswith(('::Options', '::InboxConfig'))):
        return {
            'current_usage': path + '::default().configure(|value| { /* assign supported fields */ })',
            'reason': 'This configuration is extensible: non_exhaustive blocks downstream exhaustive literals while Default and synchronous configure preserve field customization when optional fields are added.',
            'replacement': path + '::default()/configure; public field reads and assignment remain supported.',
            'compatibility': 'External complete struct literals, including update syntax, migrate to default/constructors; destructuring needs .. . Existing field meanings remain contracts.',
        }
    if path.endswith(('::SessionError', '::SessionReadError', '::Decl', '::DeclKind')):
        return {
            'current_usage': 'Match the documented variants of ' + path + ' with a fallback arm.',
            'reason': 'Allow additional authentication/read failures or declaration categories without forcing exhaustive downstream matches; existing known-variant behavior is preserved.',
            'replacement': 'Keep known variant arms and add _ => ...; construction of existing variants remains available.',
            'compatibility': 'Existing exhaustive downstream matches require a fallback arm because the enum is now non_exhaustive.',
        }
    if '::runner::' in path or '::testing::' in path:
        return {
            'current_usage': 'Use inferred sealed HostedShell for ' + path + '; Harness defaults to ordinary Shell and accepts ProtectedShell explicitly.',
            'reason': 'Share runner/harness orchestration across ordinary and protected hosts without exposing an untyped registration conversion or permitting downstream host implementations.',
            'replacement': 'Prefer inference; run_app::<State, _>(...) adds the inferred host parameter, and Harness<ProtectedShell> selects protected hosting.',
            'compatibility': 'Ordinary inferred calls and default Harness annotations remain supported; explicit generic argument lists may need the host parameter.',
        }
    raise ValueError('Changed API has no specific reviewed decision: ' + key)


def build(before, after):
    old, new = contracts(before), contracts(after)
    traits = {key.split('#')[0] for key in old.keys() | new.keys() if key.endswith('#trait')}
    records = {}
    references = {}
    consumer_files = sorted({
        *(ROOT / 'crates/fairing/tests').rglob('*.rs'),
        *(ROOT / 'crates/fairing/examples').rglob('*.rs'),
        *(ROOT / 'verification/src').rglob('*.rs'),
        ROOT / 'tools/package_consumer.rs',
    })
    for file in consumer_files:
        for number, line in enumerate(file.read_text().splitlines(), 1):
            for name in set(re.findall(r'\b[A-Za-z_]\w*\b', line)):
                candidates = references.setdefault(name, [])
                if len(candidates) < 3:
                    candidates.append(f'{file.relative_to(ROOT)}:{number}: {line.strip()}')
    for key in sorted(old.keys() | new.keys()):
        previous, current = old.get(key), new.get(key)
        group = category(key, traits)
        if current is None:
            decision, alternate = 'remove', replacement(key)
        elif previous is None:
            decision, alternate = 'add', key.split('#')[0]
        elif previous['contracts'] != current['contracts']:
            decision, alternate = 'change', key.split('#')[0] + '; see the migration guide'
        else:
            decision, alternate = 'retain', key.split('#')[0]
        kind = key.rsplit('#', 1)[1]
        syntax = {'function': 'Call the exported function/method with its compiler-recorded signature.',
                  'struct_field': 'Read/write the field as permitted by its containing public type and borrow; a shared Shell observer does not grant mutable component access.',
                  'trait': 'Implement the documented application extension contract, unless classified as sealed.',
                  'variant': 'Construct/match this variant, using a fallback when its parent enum is non-exhaustive.',
                  'struct': 'Construct through the documented constructor/default; use public field literals only for exhaustive value models.',
                  'enum': 'Construct/match declared variants, respecting non-exhaustive attributes.',
                  'use': 'Use this reexport; foreign glob bodies remain upstream dependencies.'}.get(kind, 'Use the exported declaration according to its recorded kind and signature.')
        records[key] = {
            'decision': decision, 'category': group, 'current_usage': ('Removed; migrate the previous usage. ' if current is None else '') + syntax,
            'reason': RATIONALE[group], 'replacement': alternate,
            'compatibility': ('Source migration required before release.' if decision in {'remove', 'change'} else
                              'Additive public contract.' if decision == 'add' else 'Preserve the existing source contract.'),
            'before': previous, 'after': current,
            'consumer_reference_candidates': references.get(key.split('#')[0].rsplit('::', 1)[-1], []),
        }
        records[key].update(change_details(key, decision))
    return {'baseline_head': before['head'], 'extractor_sha256': after['extractor_sha256'],
            'consumer_evidence_scope': 'Reference candidates are literal identifier matches in repository tests, examples, assurance clients and the standalone package consumer. They aid review and do not establish type resolution or per-API test coverage. Executed external consumer profiles are recorded separately in PR evidence.',
            'compiler': after['compiler'], 'decisions': records}


def check(ledger, current):
    ledger = expanded(ledger)
    if ledger['extractor_sha256'] != current['extractor_sha256'] or ledger['compiler'] != current['compiler']:
        raise ValueError('Use the reviewed compiler and extractor for the decision check')
    expected = {key: value['after'] for key, value in ledger['decisions'].items() if value['after'] is not None}
    observed = contracts(current)
    differences = sorted(key for key in expected.keys() | observed.keys() if expected.get(key) != observed.get(key))
    if differences:
        raise ValueError('Public contracts lack a current decision: ' + ', '.join(differences[:20]))
    for key in observed:
        path = key.split('#')[0]
        if '::Shell::' in path and path.rsplit('::', 1)[-1] in MUTABLE_LOANS:
            raise ValueError('Mutable Shell component loan reintroduced: ' + key)
    for key, record in ledger['decisions'].items():
        if not all(record.get(field) for field in ('decision', 'category', 'current_usage', 'reason', 'replacement', 'compatibility')):
            raise ValueError('Incomplete API decision: ' + key)
    return {'canonical_decisions': len(ledger['decisions']), 'current_contracts': len(observed),
            'exported_contracts': {profile: len(value['items']) for profile, value in current['profiles'].items()},
            'status': 'matched'}


def compact(ledger):
    """Store repeated explanations once; each API retains an explicit reference."""
    result = {key: value for key, value in ledger.items() if key != 'decisions'}
    result['shared_text'] = {'reason': {}, 'current_usage': {}, 'compatibility': {}}
    result['decisions'] = {}
    for key, record in ledger['decisions'].items():
        stored = dict(record)
        specific = record['decision'] in {'change', 'remove'}
        for field, reference in [('reason', key if specific else record['category']),
                                 ('current_usage', key if specific else key.rsplit('#', 1)[1]),
                                 ('compatibility', key if specific else record['decision'])]:
            table = result['shared_text'][field]
            if reference in table and table[reference] != stored[field]:
                raise ValueError('Inconsistent shared decision text: ' + reference)
            table[reference] = stored.pop(field)
            stored[field + '_ref'] = reference
        if stored['before'] == stored['after']:
            stored['before'] = 'same-as-after'
        result['decisions'][key] = stored
    return result


def expanded(ledger):
    if 'shared_text' not in ledger:
        return ledger
    result = {key: value for key, value in ledger.items() if key not in ('shared_text', 'decisions')}
    result['decisions'] = {}
    for key, record in ledger['decisions'].items():
        restored = dict(record)
        for field in ('reason', 'current_usage', 'compatibility'):
            restored[field] = ledger['shared_text'][field][restored.pop(field + '_ref')]
        if restored['before'] == 'same-as-after':
            restored['before'] = restored['after']
        result['decisions'][key] = restored
    return result


def encode(ledger):
    """One canonical API per line keeps intentional contract changes reviewable."""
    header = json.dumps({key: value for key, value in ledger.items() if key != 'decisions'}, indent=2)
    entries = list(ledger['decisions'].items())
    lines = [header[:-2] + ',', '  "decisions": {']
    for index, (key, record) in enumerate(entries):
        lines.append('    ' + json.dumps(key) + ': ' + json.dumps(record, separators=(',', ':'))
                     + (',' if index + 1 < len(entries) else ''))
    return '\n'.join(lines + ['  }', '}', ''])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inventory', type=Path, default=ROOT / 'target/api-surface/summary.json')
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--write', action='store_true', help='Explicitly replace the reviewed ledger; inspect its diff before approval')
    args = parser.parse_args()
    current = json.loads(args.inventory.read_text())
    if args.write:
        if args.baseline is None:
            parser.error('--write requires the frozen baseline inventory')
        LEDGER.write_text(encode(compact(build(json.loads(args.baseline.read_text()), current))))
    result = check(json.loads(LEDGER.read_text()), current)
    (args.inventory.parent / 'decisions-check.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
