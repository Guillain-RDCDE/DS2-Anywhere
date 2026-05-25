#!/bin/bash
# Audio conversion cron — runs every minute via /etc/cron.d/audio_converter.
# Two modes per pass:
#
#   MODE 1 (legacy queue) — consume <SRC>/ (default: audio_aconv/)
#     - drains *.ds2/*.dss files dropped by upstream intake scripts
#     - on success: REMOVES source (it's a queue staging folder)
#     - anti-doublon: if destination MP3 is fresh (<10 min), just cleanup
#
#   MODE 2 (auto-feed from mail/) — DB-driven, scans active commands
#     - SELECT message_id WHERE subject has .ds2/.dss and status is active and <= 7 days
#     - look in <DST_MAIL_BASE>/m{prefix}/{cmd_id}/ for top-level *.ds2/*.dss
#     - if found AND no audio file already next to it → convert + distribute
#     - DOES NOT REMOVE the source DS2 (client's original upload)
#
#     Mode 2 is SKIPPED entirely if USE_DB=0 (set via config file or --no-db flag).
#
# Configuration:
#   The script sources /etc/conv-dss-ds2-to-mp3/audio-cron.conf if present,
#   overriding any of the defaults below. Set $AUDIO_CRON_CONF to point elsewhere.
#
# Safety nets:
#   - flock (cron-level) prevents overlapping passes
#   - mtime stability check (skip if modified <60s ago)
#   - status-based filter (skip livred/delivered/cancelled)
#   - encrypted DS2 without password → leave + alert
#   - conversion failure → leave source + alert
#
# Exit code: always 0 (per-file errors handled unitarily, never crash the pass).
#
# Usage:
#   audio_cron.sh             normal pass (live)
#   audio_cron.sh --dry-run   simulate, touch nothing
#   audio_cron.sh --no-db     skip mode 2 even if USE_DB=1 in config

set -u
shopt -s nocaseglob nullglob

# --- Source config file if present (overrides defaults below) ---
CONF=${AUDIO_CRON_CONF:-/etc/conv-dss-ds2-to-mp3/audio-cron.conf}
# shellcheck source=/dev/null
[ -f "$CONF" ] && . "$CONF"

# --- Defaults (used if not set by config) ---
: "${AUDIO_ROOT_PREFIX:=/srv/AUDIO_ROOT}"
: "${SRC:=$AUDIO_ROOT_PREFIX/ftpsaisieaudio/faudio/audio_aconv}"
: "${DST_MAIL_BASE:=$AUDIO_ROOT_PREFIX/ftpmadactylo/mail}"
: "${DST_FTP_BASE:=$AUDIO_ROOT_PREFIX/ftpsaisieaudio/41}"
: "${DST_STT_BASE:=$AUDIO_ROOT_PREFIX/ftpsaisieaudio/tmp_audio_speechtotext}"
: "${LOG_FILE:=/var/log/audio_converter.log}"
: "${DB_HOST:=127.0.0.1}"
: "${DB_USER:=DB_USER_PLACEHOLDER}"
: "${DB_PASS:=DB_PASSWORD_PLACEHOLDER}"
: "${DB_NAME:=DB_NAME_PLACEHOLDER}"
: "${ALERT_EMAIL:=}"
: "${CLI:=/usr/local/bin/conv-dss-ds2-to-mp3}"
: "${USE_DB:=1}"

# --- Argument parsing ---
DRYRUN=0
for a in "$@"; do
  case "$a" in
    --dry-run) DRYRUN=1 ;;
    --no-db)   USE_DB=0 ;;
    *)         echo "Unknown flag: $a" >&2 ;;
  esac
done

mkdir -p "$(dirname "$LOG_FILE")"
TMPDIR="$(mktemp -d -t audio_cron.XXXXXX)"
trap 'rm -rf "$TMPDIR"' EXIT

log() { echo "[$(date '+%F %T')] $*" | tee -a "$LOG_FILE"; }

log "=== audio_cron pass (dryrun=$DRYRUN, use_db=$USE_DB) ==="

# ============================================================
# Helpers
# ============================================================

compute_stem() {
  local fname="$1"
  local cmd_id="$2"
  local stem
  if [[ "$fname" == *###* ]]; then
    local after="${fname##*\#\#\#}"
    stem="${after%.*}"
  else
    local nopfx="${fname#"${cmd_id}_"}"
    stem="${nopfx%.*}"
  fi
  [ -z "$stem" ] && stem="$cmd_id"
  printf '%s' "$stem"
}

send_alert() {
  # send_alert "subject" "body" — no-op if ALERT_EMAIL is empty
  [ -z "$ALERT_EMAIL" ] && return 0
  printf '%s\n' "$2" | mail -s "$1" "$ALERT_EMAIL" 2>/dev/null || true
}

convert_and_distribute() {
  # args: src cmd_id stem label  →  0 ok / 2 encrypted / 3 convert fail / 4 distribute fail
  local src="$1" cmd_id="$2" stem="$3" label="$4"
  local prefix="${cmd_id::-4}"
  local dest_mail="$DST_MAIL_BASE/m${prefix}/${cmd_id}"
  local dest_ftp="$DST_FTP_BASE/${cmd_id}"
  local dest_stt="$DST_STT_BASE/m${prefix}/${cmd_id}"
  local out_mp3="$TMPDIR/${cmd_id}_${stem}.mp3"

  local info encryption t0 t1 dest ok_dist=1
  if ! info="$("$CLI" --inspect "$src" 2>&1)"; then
    log "[$cmd_id $label] inspect failed: $info"
    return 3
  fi
  encryption="$(printf '%s\n' "$info" | awk -F: '/encryption/ {gsub(/^[ \t]+|[ \t]+$/,"",$2); print $2}')"
  if [ -n "$encryption" ] && [ "$encryption" != "none" ]; then
    log "[$cmd_id $label] ENCRYPTED ($encryption) — alert sent"
    send_alert "[ds2-anywhere] $cmd_id encrypted ($label)" \
      "Cmd $cmd_id : encrypted DS2 ($encryption) detected by audio_cron ($label).
Source: $src
Needs password — handle via the admin convertisseur page."
    return 2
  fi

  t0=$(date +%s)
  if ! "$CLI" "$src" "$out_mp3" >> "$LOG_FILE" 2>&1; then
    log "[$cmd_id $label] CONVERSION FAILED"
    send_alert "[ds2-anywhere] $cmd_id FAIL ($label)" \
      "Cmd $cmd_id : DS2 conversion failed ($label).
Source: $src
Detail in $LOG_FILE"
    return 3
  fi
  t1=$(date +%s)

  for dest in "$dest_mail" "$dest_ftp" "$dest_stt"; do
    if ! mkdir -p "$dest"; then
      log "[$cmd_id $label] mkdir $dest failed"; ok_dist=0; break
    fi
    chmod 777 "$dest" 2>/dev/null || true
    if ! cp "$out_mp3" "$dest/${stem}.mp3"; then
      log "[$cmd_id $label] cp to $dest failed"; ok_dist=0; break
    fi
    chown www-data:www-data "$dest/${stem}.mp3" 2>/dev/null || true
    chmod 666 "$dest/${stem}.mp3"
  done
  [ "$ok_dist" = 1 ] || return 4

  log "[$cmd_id $label] OK -> ${stem}.mp3 (conv $((t1-t0))s)"
  return 0
}

# ============================================================
# MODE 1: consume <SRC>/  (legacy compat)
# ============================================================
log "--- mode 1: $SRC ---"

n_ok=0; n_skip=0; n_clean=0; n_fail=0; n_crypt=0

aconv_files=( "$SRC"/*.ds2 "$SRC"/*.dss )
if [ ${#aconv_files[@]} -eq 0 ]; then
  log "queue empty"
else
  for f in "${aconv_files[@]}"; do
    [ -f "$f" ] || continue
    fname="$(basename "$f")"

    if [ -n "$(find "$f" -mmin -1 -print -quit 2>/dev/null)" ]; then
      log "[$fname] aconv: modified <60s ago, will retry"
      n_skip=$((n_skip+1)); continue
    fi

    cmd_id="${fname%%_*}"
    if ! [[ "$cmd_id" =~ ^[0-9]{7}$ ]]; then
      log "[$fname] aconv: invalid cmd_id '$cmd_id', skip"
      n_skip=$((n_skip+1)); continue
    fi

    # DB status check (only if USE_DB=1)
    if [ "$USE_DB" = 1 ]; then
      status="$(mysql -h"$DB_HOST" -u"$DB_USER" -p"$DB_PASS" "$DB_NAME" -Nse \
        "SELECT statut FROM email_messages WHERE message_id='$cmd_id'" 2>/dev/null)"
      case "$status" in
        livred*|annule|faussecommande)
          log "[$cmd_id] aconv: cmd status '$status' -> leftover cleanup"
          [ "$DRYRUN" = 0 ] && rm -f "$f"
          n_clean=$((n_clean+1)); continue ;;
        "")
          log "[$cmd_id] aconv: cmd not found in DB, skip"
          n_skip=$((n_skip+1)); continue ;;
      esac
    fi

    stem="$(compute_stem "$fname" "$cmd_id")"

    # Anti-doublon: if destination MP3 is fresh (<10 min), skip + cleanup
    prefix="${cmd_id::-4}"
    mp3_dest="$DST_MAIL_BASE/m${prefix}/${cmd_id}/${stem}.mp3"
    if [ -f "$mp3_dest" ]; then
      age_min=$(( ($(date +%s) - $(stat -c%Y "$mp3_dest")) / 60 ))
      if [ "$age_min" -lt 10 ]; then
        log "[$cmd_id aconv] doublon: ${stem}.mp3 already produced ${age_min}min ago -> cleanup without reconverting"
        [ "$DRYRUN" = 0 ] && rm -f "$f"
        n_clean=$((n_clean+1)); continue
      fi
    fi

    if [ "$DRYRUN" = 1 ]; then
      log "[DRYRUN $cmd_id aconv] would convert '$fname' -> ${stem}.mp3"
      n_ok=$((n_ok+1)); continue
    fi

    if convert_and_distribute "$f" "$cmd_id" "$stem" "aconv"; then
      rm -f "$f"
      n_ok=$((n_ok+1))
    else
      case $? in
        2) n_crypt=$((n_crypt+1)) ;;
        *) n_fail=$((n_fail+1)) ;;
      esac
    fi
  done
fi

# ============================================================
# MODE 2: auto-feed from mail/  (DB-driven, only if USE_DB=1)
# ============================================================

n_feed_ok=0; n_feed_skip=0; n_feed_fail=0; n_feed_crypt=0; n_feed_noop=0

if [ "$USE_DB" != 1 ]; then
  log "--- mode 2: SKIPPED (USE_DB=0) ---"
else
  log "--- mode 2: scan mail/ ---"

  cmds="$(mysql -h"$DB_HOST" -u"$DB_USER" -p"$DB_PASS" "$DB_NAME" -Nse "
SELECT message_id FROM email_messages
WHERE (header_subject LIKE '%.DS2%' OR header_subject LIKE '%.ds2%'
    OR header_subject LIKE '%.DSS%' OR header_subject LIKE '%.dss%')
  AND statut NOT LIKE 'livred%'
  AND statut NOT IN ('annule','faussecommande','parked_alerte')
  AND header_date >= NOW() - INTERVAL 7 DAY
" 2>/dev/null)"

  if [ -z "$cmds" ]; then
    log "no active commands with DS2/DSS in the last 7 days"
  else
    for cmd_id in $cmds; do
      prefix="${cmd_id::-4}"
      D="$DST_MAIL_BASE/m${prefix}/${cmd_id}"
      [ -d "$D" ] || { log "[$cmd_id feed] mail/ missing, skip"; n_feed_skip=$((n_feed_skip+1)); continue; }

      ds_files=( "$D"/*.ds2 "$D"/*.dss )
      if [ ${#ds_files[@]} -eq 0 ]; then
        n_feed_noop=$((n_feed_noop+1)); continue
      fi

      audio_existing=( "$D"/*.mp3 "$D"/*.wav "$D"/*.m4a "$D"/*.mpeg "$D"/*.mpga "$D"/*.wma "$D"/*.ogg )
      if [ ${#audio_existing[@]} -gt 0 ]; then
        n_feed_noop=$((n_feed_noop+1)); continue
      fi

      src="${ds_files[0]}"
      fname="$(basename "$src")"

      if [ -n "$(find "$src" -mmin -1 -print -quit 2>/dev/null)" ]; then
        log "[$cmd_id feed] '$fname' touched <60s ago, will retry"
        n_feed_skip=$((n_feed_skip+1)); continue
      fi

      stem="$(compute_stem "$fname" "$cmd_id")"

      if [ "$DRYRUN" = 1 ]; then
        log "[DRYRUN $cmd_id feed] would convert '$fname' -> ${stem}.mp3 (DS2 source kept in place)"
        n_feed_ok=$((n_feed_ok+1)); continue
      fi

      if convert_and_distribute "$src" "$cmd_id" "$stem" "feed"; then
        log "[$cmd_id feed] source DS2 KEPT in mail/ (client original)"
        n_feed_ok=$((n_feed_ok+1))
      else
        case $? in
          2) n_feed_crypt=$((n_feed_crypt+1)) ;;
          *) n_feed_fail=$((n_feed_fail+1)) ;;
        esac
      fi
    done
  fi
fi

log "=== pass done: aconv[ok=$n_ok clean=$n_clean skip=$n_skip crypt=$n_crypt fail=$n_fail] feed[ok=$n_feed_ok noop=$n_feed_noop skip=$n_feed_skip crypt=$n_feed_crypt fail=$n_feed_fail] ==="
exit 0
