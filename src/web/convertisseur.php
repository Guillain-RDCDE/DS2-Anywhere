<?php
// Admin web UI — drag-and-drop a DS2/DSS file for conversion, OR unblock a
// command by its numeric ID. Talks to the local HTTP daemon on 127.0.0.1:8765
// (audio-convert.service). PHP 5.6 compatible (no null coalescing operator,
// no short-list assignment, etc. — adapt for your PHP version).
//
// Two usage patterns:
//   1. Upload a .ds2/.dss file (+ optional password) -> MP3 streams back as download.
//   2. Type a 7-digit command ID -> backend finds the DS2 in mail/ folders,
//      converts, distributes the MP3, returns a success message.
//
// Authentication: this file lives in an admin directory protected by your
// existing auth layer. The session check below uses a generic pattern;
// adapt to your auth scheme.

session_start();
require_once 'dbconnect.php';   // your DB connection helper

// --- Auth (adapt to your scheme) ---
$id = isset($_SESSION['user_id']) ? intval($_SESSION['user_id']) : 0;
$auth = false;
if ($id > 0) {
  // Example: admin_level=1 column, plus a few hardcoded user IDs allowed.
  $q = mysql_query("SELECT admin_level FROM users WHERE id=$id");
  if ($q && ($r = mysql_fetch_assoc($q))) {
    $allowed_ids = array(/* ADMIN_USER_IDS_PLACEHOLDER */);
    if ($r['admin_level'] == 1 || in_array($id, $allowed_ids)) $auth = true;
  }
}
if (!$auth) { header('Location: index.php'); exit; }

$DAEMON = 'http://127.0.0.1:8765';
$msg = '';
$msg_type = '';

function safe($s) { return htmlspecialchars((string)$s, ENT_QUOTES, 'UTF-8'); }

// === Handler 1: upload + convert + direct download ===
if ($_SERVER['REQUEST_METHOD'] === 'POST' && !empty($_FILES['ds2_file']['tmp_name']) && $_FILES['ds2_file']['error'] === UPLOAD_ERR_OK) {
  $tmp = $_FILES['ds2_file']['tmp_name'];
  $orig = $_FILES['ds2_file']['name'];
  $ext = strtolower(pathinfo($orig, PATHINFO_EXTENSION));
  if (!in_array($ext, array('ds2', 'dss'))) {
    $msg = "The file must be a .ds2 or .dss.";
    $msg_type = 'error';
  } else {
    $password = isset($_POST['password']) ? trim($_POST['password']) : '';
    $url = $DAEMON . '/convert-upload?ext=' . urlencode($ext);
    if ($password !== '') $url .= '&password=' . urlencode($password);
    $body = file_get_contents($tmp);
    $ch = curl_init($url);
    curl_setopt_array($ch, array(
      CURLOPT_POST => true,
      CURLOPT_POSTFIELDS => $body,
      CURLOPT_HTTPHEADER => array('Content-Type: application/octet-stream', 'Expect:'),
      CURLOPT_RETURNTRANSFER => true,
      CURLOPT_HEADER => true,
      CURLOPT_TIMEOUT => 900,
    ));
    $resp = curl_exec($ch);
    if ($resp === false) {
      $msg = "Communication error with the conversion service: " . safe(curl_error($ch));
      $msg_type = 'error';
      curl_close($ch);
    } else {
      $hsize = curl_getinfo($ch, CURLINFO_HEADER_SIZE);
      $code = curl_getinfo($ch, CURLINFO_HTTP_CODE);
      $hdr = substr($resp, 0, $hsize);
      $bdy = substr($resp, $hsize);
      curl_close($ch);
      if ($code === 200 && stripos($hdr, 'Content-Type: audio/mpeg') !== false) {
        $base = pathinfo($orig, PATHINFO_FILENAME);
        header('Content-Type: audio/mpeg');
        header('Content-Disposition: attachment; filename="' . $base . '.mp3"');
        header('Content-Length: ' . strlen($bdy));
        echo $bdy;
        exit;
      } else {
        $j = json_decode($bdy, true);
        if (is_array($j) && !empty($j['encrypted'])) {
          $msg = "File is encrypted (" . safe($j['encryption']) . "). Enter the password then try again.";
          $msg_type = 'warn';
        } else {
          $msg = "Conversion failed: " . safe(is_array($j) && isset($j['error']) ? $j['error'] : 'invalid response');
          $msg_type = 'error';
        }
      }
    }
  }
}

// === Handler 2: unblock by command ID ===
if ($_SERVER['REQUEST_METHOD'] === 'POST' && !empty($_POST['cmd_id'])) {
  $cmd_id = preg_replace('/\D/', '', $_POST['cmd_id']);
  if (strlen($cmd_id) !== 7) {
    $msg = "Command ID must be 7 digits.";
    $msg_type = 'error';
  } else {
    $q = mysql_query("SELECT statut FROM email_messages WHERE message_id='" . mysql_real_escape_string($cmd_id) . "'");
    $row = $q ? mysql_fetch_assoc($q) : null;
    if (!$row) {
      $msg = "Command " . safe($cmd_id) . " not found.";
      $msg_type = 'error';
    } elseif (preg_match('/^livred/', $row['statut']) || in_array($row['statut'], array('annule', 'faussecommande'))) {
      $msg = "Command " . safe($cmd_id) . ": already " . safe($row['statut']) . ", nothing to unblock.";
      $msg_type = 'warn';
    } else {
      $prefix = substr($cmd_id, 0, -4);
      $candidates = array();
      // Search audio_aconv, mail/ root, and _audio_remplaces_ archive subfolders
      $g1 = glob("/srv/AUDIO_ROOT/ftpsaisieaudio/faudio/audio_aconv/{$cmd_id}_*.{ds2,dss,DS2,DSS}", GLOB_BRACE);
      $g2 = glob("/srv/AUDIO_ROOT/ftpmadactylo/mail/m{$prefix}/{$cmd_id}/*.{ds2,dss,DS2,DSS}", GLOB_BRACE);
      $g3 = glob("/srv/AUDIO_ROOT/ftpmadactylo/mail/m{$prefix}/{$cmd_id}/_audio_remplaces_*/*.{ds2,dss,DS2,DSS}", GLOB_BRACE);
      foreach (array($g1, $g2, $g3) as $g) {
        if (is_array($g)) foreach ($g as $f) $candidates[] = $f;
      }

      if (empty($candidates)) {
        $msg = "No .ds2 or .dss file found for command " . safe($cmd_id) . ".";
        $msg_type = 'error';
      } else {
        $src = $candidates[0];
        $base = basename($src);
        if (strpos($base, '###') !== false) {
          $parts = explode('###', $base);
          $name = end($parts);
        } else {
          $name = preg_replace("/^{$cmd_id}_/", '', $base);
        }
        $stem = pathinfo($name, PATHINFO_FILENAME);
        $output_name = $stem . '.mp3';

        $password = isset($_POST['password']) ? trim($_POST['password']) : '';
        $payload = json_encode(array(
          'input' => $src,
          'output_dirs' => array(
            "/srv/AUDIO_ROOT/ftpmadactylo/mail/m{$prefix}/{$cmd_id}",
            "/srv/AUDIO_ROOT/ftpsaisieaudio/41/{$cmd_id}",
            "/srv/AUDIO_ROOT/ftpsaisieaudio/tmp_audio_speechtotext/m{$prefix}/{$cmd_id}",
          ),
          'output_name' => $output_name,
          'password' => $password !== '' ? $password : null,
        ));
        $ch = curl_init($DAEMON . '/convert-path');
        curl_setopt_array($ch, array(
          CURLOPT_POST => true,
          CURLOPT_POSTFIELDS => $payload,
          CURLOPT_HTTPHEADER => array('Content-Type: application/json'),
          CURLOPT_RETURNTRANSFER => true,
          CURLOPT_TIMEOUT => 900,
        ));
        $resp = curl_exec($ch);
        $code = curl_getinfo($ch, CURLINFO_HTTP_CODE);
        curl_close($ch);
        $j = json_decode((string)$resp, true);

        if ($code === 200 && is_array($j) && !empty($j['ok'])) {
          $dur_min = round($j['duration_s'] / 60, 1);
          $size_mb = round($j['mp3_bytes'] / 1024 / 1024, 1);
          $msg = "Command " . safe($cmd_id) . " unblocked. Audio converted (" . safe($dur_min) . " min, " . safe($size_mb) . " MB) and deposited. Whisper will pick it up within the minute.";
          $msg_type = 'success';
        } elseif (is_array($j) && !empty($j['encrypted'])) {
          $msg = "DS2 for command " . safe($cmd_id) . " is encrypted (" . safe($j['encryption']) . "). Enter the password and retry.";
          $msg_type = 'warn';
        } else {
          $err = is_array($j) && isset($j['error']) ? $j['error'] : 'invalid response';
          $msg = "Failed: " . safe($err);
          $msg_type = 'error';
        }
      }
    }
  }
}
?><!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>DS2/DSS converter</title>
<style>
  body { background:#f5f0eb; font-family: system-ui,-apple-system,Segoe UI,sans-serif; color:#222; max-width:760px; margin:30px auto; padding:0 18px; }
  h1 { color:#b8433e; font-size:1.55em; margin:0 0 6px; }
  .sub { color:#888; margin:0 0 22px; font-size:.95em; }
  .card { background:#fff; border-radius:10px; padding:22px 24px; margin:16px 0; box-shadow:0 1px 4px rgba(0,0,0,.07); }
  .card h2 { margin:0 0 4px; font-size:1.1em; color:#222; }
  .card .help { color:#666; font-size:.92em; margin:4px 0 14px; }
  label { display:block; margin:12px 0 4px; font-weight:500; font-size:.95em; }
  input[type=text],input[type=password],input[type=file] { font-size:1em; padding:9px 11px; border:1px solid #ccc; border-radius:6px; width:100%; box-sizing:border-box; background:#fafafa; }
  input[type=text]:focus,input[type=password]:focus { background:#fff; border-color:#b8433e; outline:none; }
  button { background:#b8433e; color:#fff; border:0; padding:11px 24px; font-size:1em; font-weight:500; border-radius:6px; cursor:pointer; margin-top:16px; }
  button:hover { background:#9c3631; }
  button:disabled { background:#bbb; cursor:wait; }
  .msg { padding:13px 16px; border-radius:8px; margin:14px 0 18px; font-weight:500; line-height:1.4; }
  .msg.success { background:#e7f5e9; color:#1d6f2b; border:1px solid #6fbf81; }
  .msg.warn { background:#fff7e0; color:#7a5a00; border:1px solid #f2c94c; }
  .msg.error { background:#fde7e7; color:#a31a1a; border:1px solid #e89a9a; }
  .ftn { color:#aaa; font-size:.82em; text-align:center; margin-top:28px; }
</style>
</head>
<body>
<h1>DS2/DSS &rarr; MP3 converter</h1>
<p class="sub">Internal admin tool</p>

<?php if ($msg): ?>
<div class="msg <?= safe($msg_type) ?>"><?= $msg ?></div>
<?php endif; ?>

<div class="card">
  <h2>1. Convert a file</h2>
  <p class="help">Choose a .ds2 or .dss file. The MP3 will be offered as a direct download.</p>
  <form method="POST" enctype="multipart/form-data" onsubmit="var b=this.querySelector('button');b.disabled=true;b.textContent='Converting...';">
    <label>.ds2 or .dss file</label>
    <input type="file" name="ds2_file" accept=".ds2,.dss,.DS2,.DSS" required>
    <label>Password (only if file is encrypted)</label>
    <input type="password" name="password" placeholder="(leave empty in most cases)">
    <button type="submit">Convert and download</button>
  </form>
</div>

<div class="card">
  <h2>2. Unblock a command</h2>
  <p class="help">Enter the 7-digit command ID. The DS2 will be located, converted, and deposited where the STT pipeline expects it.</p>
  <form method="POST" onsubmit="var b=this.querySelector('button');b.disabled=true;b.textContent='Processing...';">
    <label>Command ID</label>
    <input type="text" name="cmd_id" placeholder="e.g. 2940662" pattern="[0-9]{7}" maxlength="7" required>
    <label>Password (only if DS2 is encrypted)</label>
    <input type="password" name="password" placeholder="(leave empty in most cases)">
    <button type="submit">Unblock command</button>
  </form>
</div>

<div class="ftn">audio-convert local service &middot; 127.0.0.1:8765</div>
</body>
</html>
