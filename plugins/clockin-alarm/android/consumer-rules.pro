# The plugin class is created by name from Rust, its command arguments are
# filled in by Jackson, and its receivers, service and activity are named in
# the manifest: keep them all as they are.
-keep class io.github.minasskasss.clockin.alarm.** { *; }

# WorkManager (the 15-minute plan refresh) opens its Room database by
# reflection; release shrinking removed the generated constructor
# ("NoSuchMethodException: WorkDatabase_Impl.<init>").
-keep class * extends androidx.room.RoomDatabase { <init>(); }
