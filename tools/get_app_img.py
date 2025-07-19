import sys

if len(sys.argv) != 4:
    print("need 3 arguments")
    sys.exit(1)

print("Game_Cheetah_" + sys.argv[2] + "-" + sys.argv[3] + ".AppImage")
