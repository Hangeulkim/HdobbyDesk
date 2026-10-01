set sh1 to "launchctl unload -w /Library/LaunchDaemons/__APP_FULL_NAME___service.plist;"
set sh2 to "/bin/rm /Library/LaunchDaemons/__APP_FULL_NAME___service.plist;"
set sh3 to "/bin/rm /Library/LaunchAgents/__APP_FULL_NAME___server.plist;"

set sh to sh1 & sh2 & sh3
do shell script sh with prompt "__APP_NAME__ wants to unload daemon" with administrator privileges
