#!/bin/bash

SERVER="theysa@hecate"
GAMESRVERS="/mnt/mnemosyne/gameservers"
LOCAL_MAP_LOC="/var/tmp/rust-target/release/mapstique" 
LOCAL_MOD_LOC="/home/theysa/.config/VintagestoryData/Mods/mapstique.zip"
REMOTE_MOD_DIR="${GAMESRVERS}/vintagestory/VintagestoryData/Mods/"
REMOTE_MAPSTIQUE_DIR="${GAMESRVERS}/mapstique/"

# Install the mod first
/home/theysa/Development/mapstique-csharp/package.sh --install /home/theysa/.config/VintagestoryData/Mods/

# copy mapstique to the server
scp ${LOCAL_MAP_LOC} ${SERVER}:${REMOTE_MAPSTIQUE_DIR} &&
# Copy the mod to the server
scp ${LOCAL_MOD_LOC} ${SERVER}:${REMOTE_MOD_DIR}
