on run {daemon_file, agent_file, user, config_file, config2_file}
  set daemon_plist to "/Library/LaunchDaemons/__APP_FULL_NAME___service.plist"
  set agent_plist to "/Library/LaunchAgents/__APP_FULL_NAME___server.plist"
  set root_prefs to "/var/root/Library/Preferences/__APP_FULL_NAME__"
  set root_config to root_prefs & "/__APP_NAME__.toml"
  set root_config2 to root_prefs & "/__APP_NAME__2.toml"

  set validate_sources to "test -f " & quoted form of config_file & " && test ! -L " & quoted form of config_file & " && test \"$(/usr/bin/stat -f '%Su' " & quoted form of config_file & ")\" = " & quoted form of user & "; test -f " & quoted form of config2_file & " && test ! -L " & quoted form of config2_file & " && test \"$(/usr/bin/stat -f '%Su' " & quoted form of config2_file & ")\" = " & quoted form of user & ";"
  set prepare_plists to "daemon_tmp=''; agent_tmp=''; trap 'test -z \"$daemon_tmp\" || /bin/rm -f \"$daemon_tmp\"; test -z \"$agent_tmp\" || /bin/rm -f \"$agent_tmp\"' EXIT; daemon_tmp=$(/usr/bin/mktemp /Library/LaunchDaemons/.__APP_NAME___service.XXXXXX); agent_tmp=$(/usr/bin/mktemp /Library/LaunchAgents/.__APP_NAME___server.XXXXXX); /usr/bin/printf '%s' " & quoted form of daemon_file & " > \"$daemon_tmp\"; /usr/bin/printf '%s' " & quoted form of agent_file & " > \"$agent_tmp\"; /usr/sbin/chown root:wheel \"$daemon_tmp\" \"$agent_tmp\"; /bin/chmod 0644 \"$daemon_tmp\" \"$agent_tmp\";"
  set copy_configs to "/usr/bin/install -d -o root -g wheel -m 0700 " & quoted form of root_prefs & "; /usr/bin/install -o root -g wheel -m 0600 " & quoted form of config_file & " " & quoted form of root_config & "; /usr/bin/install -o root -g wheel -m 0600 " & quoted form of config2_file & " " & quoted form of root_config2 & ";"
  set publish_plists to "/bin/mv -f \"$daemon_tmp\" " & quoted form of daemon_plist & "; daemon_tmp=''; /bin/mv -f \"$agent_tmp\" " & quoted form of agent_plist & "; agent_tmp='';"
  set load_daemon to "/bin/launchctl bootstrap system " & quoted form of daemon_plist & " 2>/dev/null || /bin/launchctl load -w " & quoted form of daemon_plist & ";"

  set sh to "set -e; umask 077;" & validate_sources & prepare_plists & copy_configs & publish_plists & load_daemon

  do shell script sh with prompt "__APP_NAME__ wants to install daemon and agent" with administrator privileges
end run
