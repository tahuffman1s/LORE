# LORE's mannequins

`mannequin.glb` and `mannequin_female.glb` are the bodies every game has until it names its own model. They are CC0: use them for anything, with no credit needed. Credit is still welcome: models and animations by [Quaternius](https://quaternius.com), from the *Universal Animation Library* 1 and 2, arranged for LORE by `build_mannequin.py`.

- Bodies with realistic proportions, about 14,000 triangles, 65 bones named as Unreal's mannequin names them (`root`, `pelvis`, `spine_01`…`spine_03`, `neck_01`, `Head`, `clavicle_l`, `upperarm_l`, `lowerarm_l`, `hand_l`, fingers, `thigh_l`, `calf_l`, `foot_l`, `ball_l`, and the same with `_r`).
- 90 clips in `mannequin.glb`. The female body has none of its own: clips find bones by name, so every clip plays on both.
- A model of your own with the same bone names plays all of them. Clips named as below make it move with no script.

| What | Clips |
|---|---|
| Moving about (played by the engine) | `idle`, `walk`, `walk_back`, `walk_left`, `walk_right`, `run`, `run_back`, `run_left`, `run_right`, `sprint`, `crouch_idle`, `crouch_walk`, `crouch_back`, `crouch_left`, `crouch_right`, `jump`, `fall`, `land`, `swim`, `swim_idle`, `climb` |
| Getting about | `turn_left`, `turn_right`, `turn_around`, `roll`, `dodge_left`, `dodge_right`, `climb_up`, `climb_down`, `climb_left`, `climb_right`, `climb_ledge`, `climb_up_low`, `climb_up_high`, `vault`, `step_up` |
| Doing things | `interact`, `pick_up`, `pick_up_table`, `open_chest`, `push`, `carry`, `throw`, `drink`, `eat`, `sit_down`, `sit`, `stand_up`, `sit_talk`, `mine`, `chop`, `kneel_work` |
| People | `idle_look`, `idle_tired`, `talk`, `fold_arms`, `yes`, `no`, `celebrate`, `surprised`, `cry`, `dance`, `hold_torch`, `hold_lantern` |
| Fighting | `sword_idle`, `sword_draw`, `sword_sheathe`, `sword_attack`, `sword_attack_2`, `sword_attack_3`, `sword_heavy`, `sword_block`, `sword_dash`, `shield_idle`, `shield_bash`, `bow_draw`, `bow_aim`, `bow_aim_up`, `bow_aim_down`, `bow_shoot`, `spell_ready`, `spell_cast`, `punch`, `punch_cross`, `kick`, `hit`, `hit_head`, `knockback`, `death`, `death_back` |

To rebuild them, download the two libraries' "Unreal-Godot" exports and the female mannequin (free, CC0, from the pages named in `build_mannequin.py`), then:

```sh
python3 -I build_mannequin.py UAL1.glb UAL2.glb Mannequin_F.glb
```
