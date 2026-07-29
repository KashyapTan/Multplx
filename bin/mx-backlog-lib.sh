# shellcheck shell=bash
# Multplx backlog format and mutation library.
#
# This file is the single owner of the durable backlog markdown format.
# A backlog contains exactly one each of `## In flight`, `## Queued`, and
# `## Done`. New files use that standard order; the parser also preserves a
# complete legacy file whose sections are arranged differently.
# Item headers are `- [ ] <id> ...` or `- [x] <id> ...`.
# Body lines use two leading spaces; blank lines may separate body paragraphs.
# Column-zero item headers and section headings are the only block boundaries.
# A tab or one-space continuation is invalid and every operation refuses a
# malformed file before writing.
#
# Public functions:
#   mx_backlog_validate <file>
#   mx_backlog_list <file> <limit> <fields>
#   mx_backlog_show <file> <id>
#   mx_backlog_add <file> <id> <title> [options]
#   mx_backlog_done <file> <id> [--report p | --note s | --pr url]
#   mx_backlog_ready <file>
#   mx_backlog_hold <file> <id> --reason s --kind k
#   mx_backlog_mv <source> <destination> <id>...
#   mx_backlog_update <file> <id> --body s [--archive-body]
#   mx_backlog_block <file> <id> --by <blocker-id>
#   mx_backlog_unblock <file> <id> --by <blocker-id>
#
# The implementation is embedded here so parsing cannot drift between callers.
# Node is part of Multplx's universal toolchain and supplies reliable byte
# handling, JSON escaping, same-directory temporary writes, and rollback if the
# second rename of a two-file transaction fails.
#
# Defaults:
#   MX_BACKLOG_DONE_KEEP=10
#   MX_BACKLOG_ARCHIVE=<backlog-directory>/done-archive.md
#   config/backlog-backend=manual opts routine broker operations into the
#   documented manual path; absent or "owned" selects this library.

mx_backlog_backend_value() {
  local config_dir=$1 backend_file value
  backend_file="$config_dir/backlog-backend"
  if [ -f "$backend_file" ]; then
    value=$(tr -d '[:space:]' < "$backend_file" 2>/dev/null || true)
    [ -n "$value" ] || value=owned
    printf '%s\n' "$value"
    return 0
  fi
  printf '%s\n' owned
}

mx_backlog_backend_manual() {
  local config_dir=$1
  [ "$(mx_backlog_backend_value "$config_dir")" = manual ]
}

mx_backlog_backend_available() {
  local config_dir=$1
  ! mx_backlog_backend_manual "$config_dir"
}

mx_backlog_engine() {
  command -v node >/dev/null 2>&1 || {
    echo "mx-backlog: node is required" >&2
    return 1
  }
  node - "$@" <<'MX_BACKLOG_NODE'
'use strict';

const fs = require('fs');
const path = require('path');

const argv = process.argv.slice(2);
const action = argv.shift();
const sections = ['In flight', 'Queued', 'Done'];

function fail(message) {
  process.stderr.write(`mx-backlog: ${message}\n`);
  process.exitCode = 1;
  throw new Error('__MX_REPORTED__');
}

function records(text) {
  const out = [];
  let position = 0;
  while (position < text.length) {
    const newline = text.indexOf('\n', position);
    if (newline < 0) {
      out.push(text.slice(position));
      break;
    }
    out.push(text.slice(position, newline + 1));
    position = newline + 1;
  }
  return out;
}

function line(record) {
  return record.endsWith('\n') ? record.slice(0, -1) : record;
}

function statRegular(file, allowMissing = false) {
  let stat;
  try {
    stat = fs.lstatSync(file);
  } catch (error) {
    if (allowMissing && error.code === 'ENOENT') return null;
    fail(`cannot read backlog ${file}: ${error.message}`);
  }
  if (stat.isSymbolicLink()) fail(`backlog must not be a symlink: ${file}`);
  if (!stat.isFile()) fail(`backlog is not a regular file: ${file}`);
  return stat;
}

function parseHeader(text, section) {
  const match = text.match(/^- \[([ x])\] ([^\s]+)(?:\s+(.*))?$/);
  if (!match) fail(`invalid item header in ${section}: ${text}`);
  const id = match[2];
  if (!/^[A-Za-z0-9._-]+$/.test(id)) fail(`invalid item id: ${id}`);
  const remainder = match[3] || '';
  const metadata = {};
  for (const field of ['repo', 'kind', 'since', 'hold', 'hold-kind', 'report', 'note', 'pr']) {
    const escaped = field.replace('-', '\\-');
    const found = remainder.match(new RegExp(`\\(${escaped}: ([^)]*)\\)`));
    if (found) metadata[field.replace('-', '_')] = found[1];
  }
  const blockers = [];
  const blockerPattern = /(?:^|\s)blocked-by:\s*([A-Za-z0-9._-]+)/g;
  let blocker;
  while ((blocker = blockerPattern.exec(remainder)) !== null) blockers.push(blocker[1]);

  let title = remainder.replace(/^-+\s*/, '');
  const boundaries = [
    title.indexOf(' (repo: '),
    title.indexOf(' (kind: '),
    title.indexOf(' (since '),
    title.indexOf(' (since: '),
    title.indexOf(' (hold: '),
    title.indexOf(' (hold-kind: '),
    title.indexOf(' (report: '),
    title.indexOf(' (note: '),
    title.indexOf(' (pr: '),
    title.search(/\sblocked-by:\s*/)
  ].filter(value => value >= 0);
  if (boundaries.length) title = title.slice(0, Math.min(...boundaries));
  title = title.replace(/\s+-\s*$/, '').trim();

  return {
    id,
    checked: match[1] === 'x',
    section,
    state: section.toLowerCase().replace(' ', '_'),
    title,
    metadata,
    blockers
  };
}

function parseFile(file, options = {}) {
  const stat = statRegular(file, options.allowMissing);
  const text = stat ? fs.readFileSync(file, 'utf8') : '## In flight\n\n## Queued\n\n## Done\n';
  const recs = records(text);
  const headingIndexes = {};
  const headingOrder = [];

  for (let index = 0; index < recs.length; index += 1) {
    const textLine = line(recs[index]);
    const heading = textLine.match(/^##\s+(.+?)\s*$/);
    if (!heading) continue;
    if (!sections.includes(heading[1])) fail(`unknown backlog section "${heading[1]}" in ${file}`);
    if (headingIndexes[heading[1]] !== undefined) fail(`duplicate backlog section "${heading[1]}" in ${file}`);
    headingIndexes[heading[1]] = index;
    headingOrder.push(heading[1]);
  }
  for (const section of sections) {
    if (headingIndexes[section] === undefined) fail(`missing backlog section "## ${section}" in ${file}`);
  }
  const items = [];
  const byId = new Map();
  for (let sectionNumber = 0; sectionNumber < headingOrder.length; sectionNumber += 1) {
    const section = headingOrder[sectionNumber];
    const start = headingIndexes[section] + 1;
    const end = sectionNumber + 1 < headingOrder.length ? headingIndexes[headingOrder[sectionNumber + 1]] : recs.length;
    let index = start;
    while (index < end) {
      const textLine = line(recs[index]);
      if (textLine === '') {
        index += 1;
        continue;
      }
      if (!/^- \[[ x]\] /.test(textLine)) {
        if (/^[\t ]/.test(textLine)) {
          fail(`orphaned or non-2-space continuation at ${file}:${index + 1}: ${textLine}`);
        }
        fail(`truncated or unrecognized backlog content at ${file}:${index + 1}: ${textLine}`);
      }
      const itemStart = index;
      const header = parseHeader(textLine, section);
      index += 1;
      while (index < end && !/^- \[[ x]\] /.test(line(recs[index]))) {
        const bodyLine = line(recs[index]);
        if (bodyLine === '' || bodyLine.startsWith('  ')) {
          index += 1;
          continue;
        }
        if (/^##\s+/.test(bodyLine)) break;
        fail(`non-2-space continuation at ${file}:${index + 1}: ${bodyLine}`);
      }
      const itemEnd = index;
      const item = {...header, start: itemStart, end: itemEnd};
      if (byId.has(item.id)) fail(`duplicate backlog item id "${item.id}" in ${file}`);
      items.push(item);
      byId.set(item.id, item);
    }
  }
  return {file, stat, text, records: recs, headingIndexes, items, byId};
}

function bodyParts(parsed, item) {
  const content = parsed.records.slice(item.start + 1, item.end);
  let bodyEnd = content.length;
  while (bodyEnd > 0 && line(content[bodyEnd - 1]) === '') bodyEnd -= 1;
  return {
    bodyRecords: content.slice(0, bodyEnd),
    separators: content.slice(bodyEnd)
  };
}

function bodyText(parsed, item) {
  return bodyParts(parsed, item).bodyRecords.map(record => {
    const textLine = line(record);
    return textLine === '' ? '' : textLine.slice(2);
  }).join('\n');
}

function canonicalBody(body) {
  if (!body) return [];
  return String(body).split('\n').map(value => value === '' ? '\n' : `  ${value}\n`);
}

function itemBlock(parsed, item) {
  const parts = bodyParts(parsed, item);
  return [parsed.records[item.start], ...parts.bodyRecords];
}

function replaceRange(parsed, start, end, replacement) {
  return [
    ...parsed.records.slice(0, start),
    ...replacement,
    ...parsed.records.slice(end)
  ].join('');
}

function removeItems(parsed, selected) {
  const ranges = [...selected].map(id => parsed.byId.get(id)).sort((a, b) => b.start - a.start);
  let recs = parsed.records.slice();
  for (const item of ranges) recs.splice(item.start, item.end - item.start);
  for (let index = 0; index + 1 < recs.length; index += 1) {
    if (/^##\s+/.test(line(recs[index])) && /^##\s+/.test(line(recs[index + 1]))) {
      recs.splice(index + 1, 0, '\n');
      index += 1;
    }
  }
  return recs.join('');
}

function sectionInsertIndex(parsed, section) {
  const start = parsed.headingIndexes[section];
  const laterHeadings = Object.values(parsed.headingIndexes).filter(index => index > start);
  const end = laterHeadings.length ? Math.min(...laterHeadings) : parsed.records.length;
  let insertion = end;
  while (insertion > parsed.headingIndexes[section] + 1 && line(parsed.records[insertion - 1]) === '') {
    insertion -= 1;
  }
  return insertion;
}

function insertBlocks(parsed, section, blocks) {
  const insertion = sectionInsertIndex(parsed, section);
  const normalized = [];
  for (const block of blocks) {
    for (let index = 0; index < block.length; index += 1) {
      let record = block[index];
      if (!record.endsWith('\n')) record += '\n';
      normalized.push(record);
    }
  }
  if (insertion >= parsed.records.length || line(parsed.records[insertion]) !== '') {
    normalized.push('\n');
  }
  return [
    ...parsed.records.slice(0, insertion),
    ...normalized,
    ...parsed.records.slice(insertion)
  ].join('');
}

function insertBlocksAtSectionStart(parsed, section, blocks) {
  const insertion = parsed.headingIndexes[section] + 1;
  const normalized = [];
  for (const block of blocks) {
    for (let record of block) {
      if (!record.endsWith('\n')) record += '\n';
      normalized.push(record);
    }
  }
  if (insertion >= parsed.records.length || line(parsed.records[insertion]) !== '') {
    normalized.push('\n');
  }
  return [
    ...parsed.records.slice(0, insertion),
    ...normalized,
    ...parsed.records.slice(insertion)
  ].join('');
}

function acquireLocks(files) {
  const locks = [];
  for (const file of [...new Set(files.map(value => path.resolve(value)))].sort()) {
    const lock = `${file}.mx-lock`;
    try {
      fs.mkdirSync(lock, {mode: 0o700});
    } catch (error) {
      fail(`backlog is busy: ${file}`);
    }
    locks.push(lock);
  }
  return locks;
}

function releaseLocks(locks) {
  for (const lock of locks.reverse()) {
    try { fs.rmdirSync(lock); } catch (_) {}
  }
}

function writeTemp(file, content, mode) {
  fs.mkdirSync(path.dirname(file), {recursive: true});
  const temporary = path.join(path.dirname(file), `.${path.basename(file)}.tmp.${process.pid}.${Date.now()}`);
  fs.writeFileSync(temporary, content, {mode: mode || 0o644, flag: 'wx'});
  return temporary;
}

function atomicWrite(file, content, oldStat) {
  const temporary = writeTemp(file, content, oldStat ? oldStat.mode & 0o777 : 0o644);
  try {
    fs.renameSync(temporary, file);
  } finally {
    try { fs.unlinkSync(temporary); } catch (_) {}
  }
}

function writeTwoWithRollback(firstParsed, firstContent, secondParsed, secondContent) {
  const first = firstParsed.file;
  const second = secondParsed.file;
  const firstTemporary = writeTemp(first, firstContent, firstParsed.stat ? firstParsed.stat.mode & 0o777 : 0o644);
  const secondTemporary = writeTemp(second, secondContent, secondParsed.stat ? secondParsed.stat.mode & 0o777 : 0o644);
  let firstRenamed = false;
  try {
    fs.renameSync(firstTemporary, first);
    firstRenamed = true;
    fs.renameSync(secondTemporary, second);
  } catch (error) {
    if (firstRenamed) atomicWrite(first, firstParsed.text, firstParsed.stat);
    fail(`two-file backlog transaction failed and was rolled back: ${error.message}`);
  } finally {
    for (const temporary of [firstTemporary, secondTemporary]) {
      try { fs.unlinkSync(temporary); } catch (_) {}
    }
  }
}

function archivePath(backlog) {
  return process.env.MX_BACKLOG_ARCHIVE || path.join(path.dirname(backlog), 'done-archive.md');
}

function appendArchive(existing, heading, content) {
  let out = existing || '# Backlog archive\n';
  if (!out.endsWith('\n')) out += '\n';
  if (!out.endsWith('\n\n')) out += '\n';
  out += `## ${heading}\n\n${content}`;
  if (!out.endsWith('\n')) out += '\n';
  return out;
}

function csv(value) {
  const text = String(value);
  return /[",\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

function showItem(parsed, item) {
  const metadata = item.metadata;
  const body = bodyText(parsed, item);
  const blockers = item.blockers.join(',');
  process.stdout.write(`${item.id}:\n`);
  process.stdout.write(`  state: ${item.state}\n`);
  process.stdout.write(`  title: ${item.title}\n`);
  process.stdout.write(`  kind: ${metadata.kind || ''}\n`);
  process.stdout.write(`  repo: ${metadata.repo || ''}\n`);
  process.stdout.write(`  held: ${item.state === 'queued' && metadata.hold ? 'yes' : 'no'}\n`);
  process.stdout.write(`  blocked: ${blockers ? 'yes' : 'no'}\n`);
  process.stdout.write(`  blocked_by: ${blockers.includes(',') ? JSON.stringify(blockers) : blockers}\n`);
  process.stdout.write(`  hold_kind: ${metadata.hold_kind || ''}\n`);
  process.stdout.write(`  hold_reason: ${metadata.hold || ''}\n`);
  process.stdout.write(`  body: ${JSON.stringify(body)}\n`);
}

function mutateOne(file, callback) {
  const locks = acquireLocks([file]);
  try {
    const parsed = parseFile(file);
    const result = callback(parsed);
    if (result !== parsed.text) atomicWrite(file, result, parsed.stat);
  } finally {
    releaseLocks(locks);
  }
}

function optionValue(args, name, required = false) {
  const index = args.indexOf(name);
  if (index < 0) {
    if (required) fail(`${name} is required`);
    return '';
  }
  if (index + 1 >= args.length || args[index + 1].startsWith('--')) fail(`${name} requires a value`);
  return args[index + 1];
}

function run() {
  if (!action) fail('an operation is required');
  if (action === 'validate') {
    parseFile(argv[0]);
    return;
  }
  if (action === 'list') {
    const [file, rawLimit, fields = 'blocked_by,hold_kind,hold_reason'] = argv;
    const parsed = parseFile(file);
    const limit = Number(rawLimit);
    if (!Number.isInteger(limit) || limit < 1) fail('list limit must be a positive integer');
    const chosen = parsed.items.slice(0, limit);
    process.stdout.write(`tasks[${chosen.length}]{id,state,kind,repo,title,blocked_by,hold_kind,hold_reason}:\n`);
    for (const item of chosen) {
      const row = [
        item.id,
        item.state,
        item.metadata.kind || '-',
        item.metadata.repo || '-',
        item.title,
        item.blockers.length ? item.blockers.join(',') : 'none',
        item.metadata.hold_kind || '-',
        item.metadata.hold || '-'
      ];
      process.stdout.write(`  ${row.map(csv).join(',')}\n`);
    }
    if (parsed.items.length > chosen.length) {
      process.stdout.write(`(truncated ${parsed.items.length - chosen.length} item(s))\n`);
    }
    void fields;
    return;
  }
  if (action === 'show') {
    const [file, id] = argv;
    const parsed = parseFile(file);
    const item = parsed.byId.get(id);
    if (!item) fail(`backlog item not found: ${id}`);
    showItem(parsed, item);
    return;
  }
  if (action === 'ready') {
    const [file] = argv;
    const parsed = parseFile(file);
    const done = new Set(parsed.items.filter(item => item.state === 'done').map(item => item.id));
    const ready = parsed.items.filter(item =>
      item.state === 'queued' &&
      !item.metadata.hold &&
      item.blockers.every(blocker => done.has(blocker))
    );
    for (const item of ready) process.stdout.write(`${item.id}\n`);
    return;
  }
  if (action === 'add') {
    const [file, id, title, ...args] = argv;
    if (!/^[A-Za-z0-9._-]+$/.test(id || '')) fail(`invalid item id: ${id || ''}`);
    if (!title || /[\r\n]/.test(title)) fail('title must be one non-empty line');
    const repo = optionValue(args, '--repo') || 'broker';
    const kind = optionValue(args, '--kind') || 'delivery';
    const body = optionValue(args, '--body');
    const start = args.includes('--start');
    const blockers = [];
    for (let index = 0; index < args.length; index += 1) {
      if (args[index] === '--blocked-by') {
        const blocker = args[index + 1];
        if (!blocker || !/^[A-Za-z0-9._-]+$/.test(blocker)) fail('--blocked-by requires a valid id');
        blockers.push(blocker);
      }
    }
    mutateOne(file, parsed => {
      if (parsed.byId.has(id)) fail(`backlog item already exists: ${id}`);
      const section = start ? 'In flight' : 'Queued';
      const header = `- [ ] ${id} - ${title}${blockers.map(value => ` blocked-by: ${value}`).join('')} (repo: ${repo}) (kind: ${kind})\n`;
      return insertBlocks(parsed, section, [[header, ...canonicalBody(body)]]);
    });
    return;
  }
  if (action === 'hold') {
    const [file, id, ...args] = argv;
    const reason = optionValue(args, '--reason', true);
    const kind = optionValue(args, '--kind', true);
    if (/[\r\n()]/.test(reason)) fail('hold reason must be one line without parentheses');
    mutateOne(file, parsed => {
      const item = parsed.byId.get(id);
      if (!item) fail(`backlog item not found: ${id}`);
      if (item.state === 'done') fail(`cannot hold done item: ${id}`);
      let header = line(parsed.records[item.start]);
      header = header.replace(/\s+\(hold: [^)]*\)/g, '').replace(/\s+\(hold-kind: [^)]*\)/g, '');
      header += ` (hold: ${reason}) (hold-kind: ${kind})`;
      return replaceRange(parsed, item.start, item.start + 1, [`${header}\n`]);
    });
    return;
  }
  if (action === 'update') {
    const [file, id, ...args] = argv;
    const body = optionValue(args, '--body', true);
    const archiveBody = args.includes('--archive-body');
    const locks = acquireLocks(archiveBody ? [file, archivePath(file)] : [file]);
    try {
      const parsed = parseFile(file);
      const item = parsed.byId.get(id);
      if (!item) fail(`backlog item not found: ${id}`);
      const parts = bodyParts(parsed, item);
      const replacement = [parsed.records[item.start], ...canonicalBody(body), ...parts.separators];
      const next = replaceRange(parsed, item.start, item.end, replacement);
      if (!archiveBody) {
        atomicWrite(file, next, parsed.stat);
        return;
      }
      const oldBody = bodyText(parsed, item);
      const archive = archivePath(file);
      const archiveStat = statRegular(archive, true);
      const archiveText = archiveStat ? fs.readFileSync(archive, 'utf8') : '';
      const archived = appendArchive(
        archiveText,
        `Superseded body: ${id} (${new Date().toISOString()})`,
        canonicalBody(oldBody).join('')
      );
      const archiveParsed = {file: archive, stat: archiveStat, text: archiveText};
      writeTwoWithRollback(parsed, next, archiveParsed, archived);
    } finally {
      releaseLocks(locks);
    }
    return;
  }
  if (action === 'unblock') {
    const [file, id, ...args] = argv;
    const blocker = optionValue(args, '--by', true);
    if (process.env.MX_BACKLOG_TEST_FAIL_UNBLOCK_ID === id) {
      const marker = process.env.MX_BACKLOG_TEST_FAIL_UNBLOCK_ONCE_FILE || '';
      if (!marker || !fs.existsSync(marker)) {
        if (marker) fs.writeFileSync(marker, 'failed once\n', {mode: 0o600});
        fail(`injected unblock failure for ${id}`);
      }
    }
    mutateOne(file, parsed => {
      const item = parsed.byId.get(id);
      if (!item) fail(`backlog item not found: ${id}`);
      if (!item.blockers.includes(blocker)) fail(`${id} is not blocked by ${blocker}`);
      let header = line(parsed.records[item.start]);
      const escaped = blocker.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      header = header.replace(new RegExp(`\\s+blocked-by:\\s*${escaped}(?=\\s|$)`), '');
      return replaceRange(parsed, item.start, item.start + 1, [`${header}\n`]);
    });
    return;
  }
  if (action === 'block') {
    const [file, id, ...args] = argv;
    const blocker = optionValue(args, '--by', true);
    if (!/^[A-Za-z0-9._-]+$/.test(blocker)) fail('--by requires a valid blocker id');
    mutateOne(file, parsed => {
      const item = parsed.byId.get(id);
      if (!item) fail(`backlog item not found: ${id}`);
      if (item.state === 'done') fail(`cannot block done item: ${id}`);
      if (item.blockers.includes(blocker)) return parsed.text;
      const header = `${line(parsed.records[item.start])} blocked-by: ${blocker}\n`;
      return replaceRange(parsed, item.start, item.start + 1, [header]);
    });
    return;
  }
  if (action === 'done') {
    const [file, id, ...args] = argv;
    const artifactKinds = ['--report', '--note', '--pr'].filter(name => args.includes(name));
    if (artifactKinds.length > 1) fail('done accepts only one of --report, --note, or --pr');
    const artifactKind = artifactKinds[0] || '';
    const artifact = artifactKind ? optionValue(args, artifactKind, true) : '';
    const archive = archivePath(file);
    const locks = acquireLocks([file, archive]);
    try {
      const parsed = parseFile(file);
      const item = parsed.byId.get(id);
      if (!item) fail(`backlog item not found: ${id}`);
      if (item.state === 'done') return;
      let header = line(parsed.records[item.start]).replace(/^- \[ \]/, '- [x]');
      header = header.replace(/\s+\(hold: [^)]*\)/g, '').replace(/\s+\(hold-kind: [^)]*\)/g, '');
      if (artifactKind) header += ` (${artifactKind.slice(2)}: ${artifact})`;
      const block = [`${header}\n`, ...bodyParts(parsed, item).bodyRecords];
      const without = parseFileFromText(file, removeItems(parsed, new Set([id])), parsed.stat);
      let next = insertBlocksAtSectionStart(without, 'Done', [block]);
      let reparsed = parseFileFromText(file, next, parsed.stat);
      const doneItems = reparsed.items.filter(candidate => candidate.state === 'done');
      const rawKeep = process.env.MX_BACKLOG_DONE_KEEP || '10';
      const keep = Number(rawKeep);
      if (!Number.isInteger(keep) || keep < 0) fail('MX_BACKLOG_DONE_KEEP must be a non-negative integer');
      const overflow = doneItems.slice(keep);
      let archiveStat = statRegular(archive, true);
      let archiveText = archiveStat ? fs.readFileSync(archive, 'utf8') : '';
      if (overflow.length) {
        const archivedBlocks = overflow.map(candidate => itemBlock(reparsed, candidate).join('')).join('\n');
        archiveText = appendArchive(archiveText, `Archived Done (${new Date().toISOString()})`, archivedBlocks);
        next = removeItems(reparsed, new Set(overflow.map(candidate => candidate.id)));
      }
      if (overflow.length) {
        const archiveParsed = {file: archive, stat: archiveStat, text: archiveStat ? fs.readFileSync(archive, 'utf8') : ''};
        writeTwoWithRollback(parsed, next, archiveParsed, archiveText);
      } else {
        atomicWrite(file, next, parsed.stat);
      }
    } finally {
      releaseLocks(locks);
    }
    return;
  }
  if (action === 'mv') {
    const [source, destination, ...ids] = argv;
    if (!ids.length) fail('mv needs at least one item id');
    if (path.resolve(source) === path.resolve(destination)) fail('source and destination backlogs must differ');
    const locks = acquireLocks([source, destination]);
    try {
      const sourceParsed = parseFile(source);
      const destinationParsed = parseFile(destination, {allowMissing: true});
      const selected = new Set(ids);
      if (selected.size !== ids.length) fail('mv item ids must be unique');
      for (const id of selected) {
        const item = sourceParsed.byId.get(id);
        if (!item) fail(`source backlog item not found: ${id}`);
        if (destinationParsed.byId.has(id)) fail(`destination already contains item: ${id}`);
      }
      for (const item of sourceParsed.items) {
        const itemSelected = selected.has(item.id);
        for (const blocker of item.blockers) {
          const blockerInSource = sourceParsed.byId.has(blocker);
          const blockerInDestination = destinationParsed.byId.has(blocker);
          if (itemSelected && blockerInSource && !selected.has(blocker)) {
            fail(`moving ${item.id} would strand blocker ${blocker} in the source backlog`);
          }
          if (!itemSelected && selected.has(blocker)) {
            fail(`moving ${blocker} would strand dependent ${item.id} in the source backlog`);
          }
          if (itemSelected && blockerInDestination) {
            fail(`moving ${item.id} would retain a cross-backlog dependency on ${blocker}`);
          }
        }
      }
      const blocksBySection = new Map(sections.map(section => [section, []]));
      for (const item of sourceParsed.items) {
        if (selected.has(item.id)) blocksBySection.get(item.section).push(itemBlock(sourceParsed, item));
      }
      const sourceNext = removeItems(sourceParsed, selected);
      let destinationNext = destinationParsed.text;
      for (const section of sections) {
        const blocks = blocksBySection.get(section);
        if (!blocks.length) continue;
        const current = parseFileFromText(destination, destinationNext, destinationParsed.stat);
        destinationNext = insertBlocks(current, section, blocks);
      }
      writeTwoWithRollback(sourceParsed, sourceNext, destinationParsed, destinationNext);
    } finally {
      releaseLocks(locks);
    }
    return;
  }
  fail(`unknown operation: ${action}`);
}

function parseFileFromText(file, text, stat) {
  const temporary = path.join(path.dirname(file), `.${path.basename(file)}.parse.${process.pid}.${Date.now()}`);
  fs.mkdirSync(path.dirname(file), {recursive: true});
  fs.writeFileSync(temporary, text, {mode: 0o600});
  try {
    const parsed = parseFile(temporary);
    parsed.file = file;
    parsed.stat = stat;
    parsed.text = text;
    return parsed;
  } finally {
    try { fs.unlinkSync(temporary); } catch (_) {}
  }
}

try {
  run();
} catch (error) {
  if (error.message !== '__MX_REPORTED__') {
    process.stderr.write(`mx-backlog: ${error.message}\n`);
    process.exitCode = 1;
  }
}
MX_BACKLOG_NODE
}

mx_backlog_validate() {
  mx_backlog_engine validate "$1"
}

mx_backlog_list() {
  mx_backlog_engine list "$1" "$2" "${3:-blocked_by,hold_kind,hold_reason}"
}

mx_backlog_show() {
  mx_backlog_engine show "$1" "$2"
}

mx_backlog_add() {
  mx_backlog_engine add "$@"
}

mx_backlog_done() {
  mx_backlog_engine done "$@"
}

mx_backlog_ready() {
  mx_backlog_engine ready "$1"
}

mx_backlog_hold() {
  mx_backlog_engine hold "$@"
}

mx_backlog_mv() {
  mx_backlog_engine mv "$@"
}

mx_backlog_update() {
  mx_backlog_engine update "$@"
}

mx_backlog_block() {
  mx_backlog_engine block "$@"
}

mx_backlog_unblock() {
  mx_backlog_engine unblock "$@"
}
