#!/usr/bin/env node
/**
 * Гейт vault (AD-16, Story 1.3): plaintext-колонки токенов в миграциях
 * запрещены — токены площадок хранятся ТОЛЬКО шифротекстом (AES-256-GCM,
 * пара bytea token_ciphertext + token_nonce).
 *
 * Сканирует `*.sql` в `db/migrations` (после срезки SQL-комментариев `--`):
 *  - литеральные паттерны plaintext-хранения: token_plaintext, plaintext_token,
 *    token_plain, plain_token (регистронезависимо);
 *  - определение колонки с именем ровно token/api_key/apikey/secret
 *    (слово целиком — token_ciphertext/token_nonce не триггерят);
 *  - секрето-подобное имя + plaintext-тип (`token text`, `api_key varchar`,
 *    `secret jsonb`, `password uuid` и пр.).
 *
 * Коды выхода: 0 — чисто; 1 — найдены plaintext-колонки; 2 — ошибка скана.
 *
 * Запуск: node platform/scripts/vault-grep.mjs [каталог миграций]
 */

import { readdirSync, readFileSync, existsSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const LITERAL_PATTERNS = [
  'token_plaintext',
  'plaintext_token',
  'token_plain',
  'plain_token',
  'secret_plaintext',
  'plaintext_secret',
]

// Имя колонки ровно secret-подобное (граница — пробел после имени):
// `token_ciphertext bytea` и `token_nonce bytea` НЕ матчатся, а голая
// колонка `token bytea` — нарушение конвенции пары token_ciphertext+nonce.
const SECRET_COLUMN_RES = [
  /^\s*["'`]?(token|api_key|apikey|secret|password)["'`]?\s+\S/i,
  /[(,]\s*["'`]?(token|api_key|apikey|secret|password)["'`]?\s+\S/i,
]

// Секрето-подобное имя рядом с plaintext-типом в любой позиции строки
// (ловит перенос схемы в нестандартном форматировании).
// Составные имена тоже ловим: ozon_api_key text, wb_token text и т.п.
// (формальное ревью 1.3: \b не матчит внутри ozon_api_key).
const SECRET_PLAINTEXT_TYPE_RE =
  /\b\w*?(token|api_key|apikey|secret|password)\w*\b\s+(text|varchar|character varying|character|jsonb|uuid)\b/i

// Строчный SQL-комментарий -- до конца строки — не код (блочные срезаны
// по всему файлу до разбивки; ревью Story 1.3).
function stripLineComment(line) {
  return line.replace(/--.*$/, '')
}

function normalizeWhitespace(line) {
  return line.replace(/\s+/g, ' ').trim()
}

function migrationDir() {
  const arg = process.argv[2]
  if (arg) return resolve(arg)
  const here = dirname(fileURLToPath(import.meta.url)) // pult/platform/scripts
  return resolve(here, '..', '..', 'db', 'migrations') // pult/db/migrations
}

function listSqlFiles(dir) {
  if (!statSync(dir).isDirectory()) {
    return [dir]
  }
  return readdirSync(dir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.toLowerCase().endsWith('.sql'))
    .map((entry) => join(dir, entry.name))
    .sort()
}

const dir = migrationDir()
if (!existsSync(dir)) {
  console.error('[AD-16] vault-grep: каталог миграций не найден:', dir)
  process.exit(2)
}

let files
try {
  files = listSqlFiles(dir)
} catch (err) {
  console.error(`[AD-16] vault-grep: ошибка чтения каталога ${dir}: ${err.message}`)
  process.exit(2)
}

if (files.length === 0) {
  console.error('[AD-16] vault-grep: в каталоге нет *.sql — гейту нечего проверять:', dir)
  process.exit(2)
}

const violations = []

for (const file of files) {
  let content
  try {
    content = readFileSync(file, 'utf8')
  } catch (err) {
    console.error(`[AD-16] vault-grep: ошибка чтения ${file}: ${err.message}`)
    process.exit(2)
  }
  // Блочные комментарии /* */ срезаются ПО ВСЕМУ файлу до разбивки на
  // строки — иначе многострочный комментарий оставляет свои внутренние
  // строки детекторам (ложный FAIL, формальное ревью 1.3). Строчные --
  // срезаются построчно (они не переносятся через строки SQL).
  const withoutBlocks = content.replace(/\/\*[\s\S]*?\*\//g, ' ')
  const rawLines = withoutBlocks.split('\n')
  const normalizedLines = rawLines.map((line) => line.replace(/--.*$/, '')).map(normalizeWhitespace)
  normalizedLines.forEach((line, idx) => {
    const lowered = line.toLowerCase()
    const literal = LITERAL_PATTERNS.find((pattern) => lowered.includes(pattern))
    if (literal) {
      violations.push({
        file,
        line: idx + 1,
        raw: rawLines[idx]?.trim() ?? '',
        why: `plaintext-паттерн «${literal}»`,
      })
      return
    }
    if (SECRET_COLUMN_RES.some((re) => re.test(line))) {
      violations.push({
        file,
        line: idx + 1,
        raw: rawLines[idx]?.trim() ?? '',
        why: 'колонка с именем token/api_key/secret/password — токен обязан быть шифротекстом (token_ciphertext/token_nonce)',
      })
      return
    }
    if (SECRET_PLAINTEXT_TYPE_RE.test(line)) {
      violations.push({
        file,
        line: idx + 1,
        raw: rawLines[idx]?.trim() ?? '',
        why: 'секрето-подобное имя в plaintext-типе (text/varchar/jsonb/uuid)',
      })
    }
  })
}

if (violations.length > 0) {
  console.error(
    '[AD-16] vault-grep: НАРУШЕНИЕ — plaintext-колонки токенов в миграциях запрещены (только шифротекст token_ciphertext + token_nonce bytea)',
  )
  for (const v of violations) {
    console.error(`  - ${v.file.slice(dir.length + 1)}:${v.line}: ${v.why}`)
    console.error(`      ${v.raw}`)
  }
  console.error(`Итого нарушений: ${violations.length}`)
  process.exit(1)
}

console.log(`[AD-16] vault-grep: PASS — plaintext-колонок токенов нет (${files.length} миграций)`)
