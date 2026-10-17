#!/usr/bin/env bash
# The java arm's jar for X01.
#
#   tools/krt-h2h/build-java.sh [--frozen JAR] [COMMIT]
#
# COMMIT defaults to $JAVA_PIN (the J01 pin, a773a70ec).
# Without --frozen: `git -C ../freerouting worktree add --detach $X01_DIR/fr-<commit> <commit>`,
#   `./gradlew executableJar` there (no daemon, 2 workers, 2 GB heap), the jar copied to
#   $X01_DIR/java/freerouting-<commit>.jar, and the worktree removed. Never builds in
#   ../freerouting's own working tree, which another session edits.
# With --frozen JAR: copies an already built jar of that commit instead (the Java session's
#   frozen jars, ~/code/freerouting/logs/jars/freerouting-<sha>.jar); its file name must
#   carry the commit.
# Then records the jar's sha256, `java -version`, and whether the sha256 equals the KiCad
# plugin's freerouting-dev.jar (PLUGIN_JAR, read only), and routes
# fixtures/Issue015-StackOverflow.dsn headless with the registered arguments as a smoke test.
# Writes $X01_DIR/java/java-env.json. Exit 0 when the smoke test wrote a SES.
set -uo pipefail
source "$(dirname "$0")/common.sh"
FR_REPO=${FR_REPO:-$REPO_ROOT/../freerouting}
PLUGIN_JAR=${PLUGIN_JAR:-/mnt/c/Users/thebo/Documents/KiCad/10.0/3rdparty/plugins/app_freerouting_kicad-plugin/jar/freerouting-dev.jar}
JAVA_HEAP=${JAVA_HEAP:--Xmx3g}

FROZEN=""
if [[ "${1:-}" == "--frozen" ]]; then
  FROZEN=${2:?--frozen needs a jar}
  shift 2
fi
COMMIT=${1:-$JAVA_PIN}
OUTDIR="$X01_DIR/java"
JAR="$OUTDIR/freerouting-$COMMIT.jar"
mkdir -p "$OUTDIR"

if [[ -n "$FROZEN" ]]; then
  [[ "$(basename "$FROZEN")" == *"$COMMIT"* ]] ||
    { echo "frozen jar $FROZEN does not name commit $COMMIT" >&2; exit 1; }
  cp "$FROZEN" "$JAR" || exit 1
  SOURCE="frozen: $FROZEN"
else
  WT="$X01_DIR/fr-$COMMIT"
  git -C "$FR_REPO" worktree add --detach "$WT" "$COMMIT" > "$OUTDIR/worktree.log" 2>&1 ||
    { echo "worktree add failed (see $OUTDIR/worktree.log)" >&2; exit 1; }
  (cd "$WT" && ./gradlew --no-daemon --max-workers=2 -Dorg.gradle.jvmargs=-Xmx2g executableJar) \
    > "$OUTDIR/gradle.log" 2>&1
  BUILD=$?
  [[ $BUILD -eq 0 ]] && cp "$WT/build/libs/freerouting-current-executable.jar" "$JAR"
  git -C "$FR_REPO" worktree remove --force "$WT"
  [[ $BUILD -eq 0 ]] || { echo "gradle failed (see $OUTDIR/gradle.log)" >&2; exit 1; }
  SOURCE="built from a detached worktree of $FR_REPO at $(git -C "$FR_REPO" rev-parse "$COMMIT")"
fi

SMOKE="$OUTDIR/smoke"
rm -rf "$SMOKE"
mkdir -p "$SMOKE"
cp "$REPO_ROOT/fixtures/Issue015-StackOverflow.dsn" "$SMOKE/"
START=$(date +%s.%N)
(cd "$SMOKE" && timeout 1800 java "$JAVA_HEAP" -jar "$JAR" -de "$SMOKE/Issue015-StackOverflow.dsn" \
  -do "$SMOKE/Issue015-StackOverflow.ses" --gui.enabled=false \
  --router.scoring.start_ripup_costs=1500) > "$SMOKE/engine.log" 2>&1
EXIT=$?
END=$(date +%s.%N)

python3 - "$OUTDIR/java-env.json" "$JAR" "$SOURCE" "$COMMIT" "$PLUGIN_JAR" "$SMOKE" "$EXIT" \
  "$START" "$END" "$JAVA_HEAP" << 'PY'
import hashlib, json, os, subprocess, sys
out, jar, source, commit, plugin, smoke, rc, start, end, heap = sys.argv[1:11]
sha = lambda p: hashlib.sha256(open(p, "rb").read()).hexdigest()
ses = os.path.join(smoke, "Issue015-StackOverflow.ses")
version = subprocess.run(["java", "-version"], capture_output=True, text=True).stderr.strip()
plugin_sha = sha(plugin) if os.path.exists(plugin) else None
env = {
    "commit": commit,
    "jar": jar,
    "jar_source": source,
    "jar_sha256": sha(jar),
    "java_version": version.splitlines(),
    "jvm_heap": heap,
    "plugin_jar": plugin,
    "plugin_jar_sha256": plugin_sha,
    "equals_plugin_jar": plugin_sha == sha(jar) if plugin_sha else None,
    "smoke_dsn": "fixtures/Issue015-StackOverflow.dsn",
    "smoke_command": f"java {heap} -jar {jar} -de {{DSN}} -do {{SES}} --gui.enabled=false "
                     "--router.scoring.start_ripup_costs=1500",
    "smoke_exit": int(rc),
    "smoke_seconds": round(float(end) - float(start), 2),
    "smoke_ses": os.path.exists(ses) and os.path.getsize(ses) > 0,
    "smoke_ses_sha256": sha(ses) if os.path.exists(ses) else None,
}
json.dump(env, open(out, "w"), indent=2)
print(json.dumps(env, indent=1))
sys.exit(0 if env["smoke_ses"] else 1)
PY
