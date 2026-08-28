#!/bin/bash

SERVER="theysa@hecate"
GAMESRVERS="/mnt/mnemosyne/gameservers"
LOCAL_MAP_LOC="/var/tmp/rust-target/release/witchlight" 
LOCAL_MOD_LOC="/home/theysa/.config/VintagestoryData/Mods/witchlight.zip"
REMOTE_MOD_DIR="${GAMESRVERS}/vintagestory/VintagestoryData/Mods/"
REMOTE_WITCHLIGHT_DIR="${GAMESRVERS}/witchlight/"

# Install the mod first
/home/theysa/Development/witchlight-csharp/package.sh --install /home/theysa/.config/VintagestoryData/Mods/

# copy witchlight to the server
scp ${LOCAL_MAP_LOC} ${SERVER}:${REMOTE_WITCHLIGHT_DIR} &&
# Copy the mod to the server
scp ${LOCAL_MOD_LOC} ${SERVER}:${REMOTE_MOD_DIR}
