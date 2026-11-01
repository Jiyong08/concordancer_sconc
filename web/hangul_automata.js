// ============================================================================
// Hancom Office (한글과컴퓨터) 두벌식 옛글 & HNI 표준 첫가끝 통합 오토마타
// - 초성 124자 · 중성 94자 · 종성 137자 전이표 기반
// - 한컴 두벌식 옛글 규칙 지원:
//   * 'ㅏ'(k) 2회 연속 타건 -> 'ㆍ'(아래아, U+119E), 3회 연속 -> 'ᆢ'(쌍아래아, U+11A2)
//   * 'ㆍ' + 'ㅣ'(l) -> 'ㆎ'(U+11A1), 'ㆍ' + 'ㅜ'(n)/'ㅗ'(h) 등 모든 옛중성 자동 결합
//   * Shift+K -> 'ㆍ', Shift+F -> 'ㅿ'(반치음), Shift+D -> 'ㆁ'(옛이응), Shift+G -> 'ㆆ'(여린히읗)
//   * 연서(순경음): ㅂ+ㅇ -> ᄫ(ㅸ), ㅁ+ㅇ -> ᄝ(ㅱ), ㅍ+ㅇ -> ᅗ(ㆄ), ㄹ+ㅇ -> ᄛ, ㅃ+ㅇ -> ᄬ(ㅹ)
//   * 합용병서/각자병서: ㅂ+ㅅ -> ᄡ, ㅂ+ㄷ -> ᄞ, ㅂ+ㅅ+ㄱ -> ᄢ, ㅅ+ㄱ -> ᄭ, ㅇ+ㅇ -> ᅇ 등
// ============================================================================

const JAMO_REV = {"L": {"KIYEOK": 4352, "KIYEOK+KIYEOK": 4353, "NIEUN": 4354, "TIKEUT": 4355, "TIKEUT+TIKEUT": 4356, "RIEUL": 4357, "MIEUM": 4358, "PIEUP": 4359, "PIEUP+PIEUP": 4360, "SIOS": 4361, "SIOS+SIOS": 4362, "IEUNG": 4363, "CIEUC": 4364, "CIEUC+CIEUC": 4365, "CHIEUCH": 4366, "KHIEUKH": 4367, "THIEUTH": 4368, "PHIEUPH": 4369, "HIEUH": 4370, "NIEUN+KIYEOK": 4371, "NIEUN+NIEUN": 4372, "NIEUN+TIKEUT": 4373, "NIEUN+PIEUP": 4374, "TIKEUT+KIYEOK": 4375, "RIEUL+NIEUN": 4376, "RIEUL+RIEUL": 4377, "RIEUL+HIEUH": 4378, "KAPYEOUNRIEUL": 4379, "MIEUM+PIEUP": 4380, "KAPYEOUNMIEUM": 4381, "PIEUP+KIYEOK": 4382, "PIEUP+NIEUN": 4383, "PIEUP+TIKEUT": 4384, "PIEUP+SIOS": 4385, "PIEUP+SIOS+KIYEOK": 4386, "PIEUP+SIOS+TIKEUT": 4387, "PIEUP+SIOS+PIEUP": 4388, "PIEUP+SIOS+SIOS": 4389, "PIEUP+SIOS+CIEUC": 4390, "PIEUP+CIEUC": 4391, "PIEUP+CHIEUCH": 4392, "PIEUP+THIEUTH": 4393, "PIEUP+PHIEUPH": 4394, "KAPYEOUNPIEUP": 4395, "KAPYEOUNSSANGPIEUP": 4396, "SIOS+KIYEOK": 4397, "SIOS+NIEUN": 4398, "SIOS+TIKEUT": 4399, "SIOS+RIEUL": 4400, "SIOS+MIEUM": 4401, "SIOS+PIEUP": 4402, "SIOS+PIEUP+KIYEOK": 4403, "SIOS+SIOS+SIOS": 4404, "SIOS+IEUNG": 4405, "SIOS+CIEUC": 4406, "SIOS+CHIEUCH": 4407, "SIOS+KHIEUKH": 4408, "SIOS+THIEUTH": 4409, "SIOS+PHIEUPH": 4410, "SIOS+HIEUH": 4411, "CHITUEUMSIOS": 4412, "CHITUEUMSIOS+CHITUEUMSIOS": 4413, "CEONGCHIEUMSIOS": 4414, "CEONGCHIEUMSIOS+CEONGCHIEUMSIOS": 4415, "PANSIOS": 4416, "IEUNG+KIYEOK": 4417, "IEUNG+TIKEUT": 4418, "IEUNG+MIEUM": 4419, "IEUNG+PIEUP": 4420, "IEUNG+SIOS": 4421, "IEUNG+PANSIOS": 4422, "IEUNG+IEUNG": 4423, "IEUNG+CIEUC": 4424, "IEUNG+CHIEUCH": 4425, "IEUNG+THIEUTH": 4426, "IEUNG+PHIEUPH": 4427, "YESIEUNG": 4428, "CIEUC+IEUNG": 4429, "CHITUEUMCIEUC": 4430, "CHITUEUMCIEUC+CHITUEUMCIEUC": 4431, "CEONGCHIEUMCIEUC": 4432, "CEONGCHIEUMCIEUC+CEONGCHIEUMCIEUC": 4433, "CHIEUCH+KHIEUKH": 4434, "CHIEUCH+HIEUH": 4435, "CHITUEUMCHIEUCH": 4436, "CEONGCHIEUMCHIEUCH": 4437, "PHIEUPH+PIEUP": 4438, "KAPYEOUNPHIEUPH": 4439, "HIEUH+HIEUH": 4440, "YEORINHIEUH": 4441, "KIYEOK+TIKEUT": 4442, "NIEUN+SIOS": 4443, "NIEUN+CIEUC": 4444, "NIEUN+HIEUH": 4445, "TIKEUT+RIEUL": 4446, "TIKEUT+MIEUM": 43360, "TIKEUT+PIEUP": 43361, "TIKEUT+SIOS": 43362, "TIKEUT+CIEUC": 43363, "RIEUL+KIYEOK": 43364, "RIEUL+KIYEOK+KIYEOK": 43365, "RIEUL+TIKEUT": 43366, "RIEUL+TIKEUT+TIKEUT": 43367, "RIEUL+MIEUM": 43368, "RIEUL+PIEUP": 43369, "RIEUL+PIEUP+PIEUP": 43370, "RIEUL+KAPYEOUNPIEUP": 43371, "RIEUL+SIOS": 43372, "RIEUL+CIEUC": 43373, "RIEUL+KHIEUKH": 43374, "MIEUM+KIYEOK": 43375, "MIEUM+TIKEUT": 43376, "MIEUM+SIOS": 43377, "PIEUP+SIOS+THIEUTH": 43378, "PIEUP+KHIEUKH": 43379, "PIEUP+HIEUH": 43380, "SIOS+SIOS+PIEUP": 43381, "IEUNG+RIEUL": 43382, "IEUNG+HIEUH": 43383, "CIEUC+CIEUC+HIEUH": 43384, "THIEUTH+THIEUTH": 43385, "PHIEUPH+HIEUH": 43386, "HIEUH+SIOS": 43387, "YEORINHIEUH+YEORINHIEUH": 43388}, "V": {"A": 4449, "AE": 4450, "YA": 4451, "YAE": 4452, "EO": 4453, "E": 4454, "YEO": 4455, "YE": 4456, "O": 4457, "O+A": 4458, "O+AE": 4459, "O+I": 4460, "YO": 4461, "U": 4462, "U+EO": 4463, "U+E": 4464, "U+I": 4465, "YU": 4466, "EU": 4467, "EU+I": 4468, "I": 4469, "A+O": 4470, "A+U": 4471, "YA+O": 4472, "YA+YO": 4473, "EO+O": 4474, "EO+U": 4475, "EO+EU": 4476, "YEO+O": 4477, "YEO+U": 4478, "O+EO": 4479, "O+E": 4480, "O+YE": 4481, "O+O": 4482, "O+U": 4483, "YO+YA": 4484, "YO+YAE": 4485, "YO+YEO": 4486, "YO+O": 4487, "YO+I": 4488, "U+A": 4489, "U+AE": 4490, "U+EO+EU": 4491, "U+YE": 4492, "U+U": 4493, "YU+A": 4494, "YU+EO": 4495, "YU+E": 4496, "YU+YEO": 4497, "YU+YE": 4498, "YU+U": 4499, "YU+I": 4500, "EU+U": 4501, "EU+EU": 4502, "EU+I+U": 4503, "I+A": 4504, "I+YA": 4505, "I+O": 4506, "I+U": 4507, "I+EU": 4508, "I+ARAEA": 4509, "ARAEA": 4510, "ARAEA+EO": 4511, "ARAEA+U": 4512, "ARAEA+I": 4513, "ARAEA+ARAEA": 4514, "A+EU": 4515, "YA+U": 4516, "YEO+YA": 4517, "O+YA": 4518, "O+YAE": 4519, "O+YEO": 55216, "O+O+I": 55217, "YO+A": 55218, "YO+AE": 55219, "YO+EO": 55220, "U+YEO": 55221, "U+I+I": 55222, "YU+AE": 55223, "YU+O": 55224, "EU+A": 55225, "EU+EO": 55226, "EU+E": 55227, "EU+O": 55228, "I+YA+O": 55229, "I+YAE": 55230, "I+YEO": 55231, "I+YE": 55232, "I+O+I": 55233, "I+YO": 55234, "I+YU": 55235, "I+I": 55236, "ARAEA+A": 55237, "ARAEA+E": 55238}, "T": {"KIYEOK": 4520, "KIYEOK+KIYEOK": 4521, "KIYEOK+SIOS": 4522, "NIEUN": 4523, "NIEUN+CIEUC": 4524, "NIEUN+HIEUH": 4525, "TIKEUT": 4526, "RIEUL": 4527, "RIEUL+KIYEOK": 4528, "RIEUL+MIEUM": 4529, "RIEUL+PIEUP": 4530, "RIEUL+SIOS": 4531, "RIEUL+THIEUTH": 4532, "RIEUL+PHIEUPH": 4533, "RIEUL+HIEUH": 4534, "MIEUM": 4535, "PIEUP": 4536, "PIEUP+SIOS": 4537, "SIOS": 4538, "SIOS+SIOS": 4539, "IEUNG": 4540, "CIEUC": 4541, "CHIEUCH": 4542, "KHIEUKH": 4543, "THIEUTH": 4544, "PHIEUPH": 4545, "HIEUH": 4546, "KIYEOK+RIEUL": 4547, "KIYEOK+SIOS+KIYEOK": 4548, "NIEUN+KIYEOK": 4549, "NIEUN+TIKEUT": 4550, "NIEUN+SIOS": 4551, "NIEUN+PANSIOS": 4552, "NIEUN+THIEUTH": 4553, "TIKEUT+KIYEOK": 4554, "TIKEUT+RIEUL": 4555, "RIEUL+KIYEOK+SIOS": 4556, "RIEUL+NIEUN": 4557, "RIEUL+TIKEUT": 4558, "RIEUL+TIKEUT+HIEUH": 4559, "RIEUL+RIEUL": 4560, "RIEUL+MIEUM+KIYEOK": 4561, "RIEUL+MIEUM+SIOS": 4562, "RIEUL+PIEUP+SIOS": 4563, "RIEUL+PIEUP+HIEUH": 4564, "RIEUL+KAPYEOUNPIEUP": 4565, "RIEUL+SIOS+SIOS": 4566, "RIEUL+PANSIOS": 4567, "RIEUL+KHIEUKH": 4568, "RIEUL+YEORINHIEUH": 4569, "MIEUM+KIYEOK": 4570, "MIEUM+RIEUL": 4571, "MIEUM+PIEUP": 4572, "MIEUM+SIOS": 4573, "MIEUM+SIOS+SIOS": 4574, "MIEUM+PANSIOS": 4575, "MIEUM+CHIEUCH": 4576, "MIEUM+HIEUH": 4577, "KAPYEOUNMIEUM": 4578, "PIEUP+RIEUL": 4579, "PIEUP+PHIEUPH": 4580, "PIEUP+HIEUH": 4581, "KAPYEOUNPIEUP": 4582, "SIOS+KIYEOK": 4583, "SIOS+TIKEUT": 4584, "SIOS+RIEUL": 4585, "SIOS+PIEUP": 4586, "PANSIOS": 4587, "IEUNG+KIYEOK": 4588, "IEUNG+KIYEOK+KIYEOK": 4589, "IEUNG+IEUNG": 4590, "IEUNG+KHIEUKH": 4591, "YESIEUNG": 4592, "YESIEUNG+SIOS": 4593, "YESIEUNG+PANSIOS": 4594, "PHIEUPH+PIEUP": 4595, "KAPYEOUNPHIEUPH": 4596, "HIEUH+NIEUN": 4597, "HIEUH+RIEUL": 4598, "HIEUH+MIEUM": 4599, "HIEUH+PIEUP": 4600, "YEORINHIEUH": 4601, "KIYEOK+NIEUN": 4602, "KIYEOK+PIEUP": 4603, "KIYEOK+CHIEUCH": 4604, "KIYEOK+KHIEUKH": 4605, "KIYEOK+HIEUH": 4606, "NIEUN+NIEUN": 4607, "NIEUN+RIEUL": 55243, "NIEUN+CHIEUCH": 55244, "TIKEUT+TIKEUT": 55245, "TIKEUT+TIKEUT+PIEUP": 55246, "TIKEUT+PIEUP": 55247, "TIKEUT+SIOS": 55248, "TIKEUT+SIOS+KIYEOK": 55249, "TIKEUT+CIEUC": 55250, "TIKEUT+CHIEUCH": 55251, "TIKEUT+THIEUTH": 55252, "RIEUL+KIYEOK+KIYEOK": 55253, "RIEUL+KIYEOK+HIEUH": 55254, "RIEUL+RIEUL+KHIEUKH": 55255, "RIEUL+MIEUM+HIEUH": 55256, "RIEUL+PIEUP+TIKEUT": 55257, "RIEUL+PIEUP+PHIEUPH": 55258, "RIEUL+YESIEUNG": 55259, "RIEUL+YEORINHIEUH+HIEUH": 55260, "KAPYEOUNRIEUL": 55261, "MIEUM+NIEUN": 55262, "MIEUM+NIEUN+NIEUN": 55263, "MIEUM+MIEUM": 55264, "MIEUM+PIEUP+SIOS": 55265, "MIEUM+CIEUC": 55266, "PIEUP+TIKEUT": 55267, "PIEUP+RIEUL+PHIEUPH": 55268, "PIEUP+MIEUM": 55269, "PIEUP+PIEUP": 55270, "PIEUP+SIOS+TIKEUT": 55271, "PIEUP+CIEUC": 55272, "PIEUP+CHIEUCH": 55273, "SIOS+MIEUM": 55274, "SIOS+KAPYEOUNPIEUP": 55275, "SIOS+SIOS+KIYEOK": 55276, "SIOS+SIOS+TIKEUT": 55277, "SIOS+PANSIOS": 55278, "SIOS+CIEUC": 55279, "SIOS+CHIEUCH": 55280, "SIOS+THIEUTH": 55281, "SIOS+HIEUH": 55282, "PANSIOS+PIEUP": 55283, "PANSIOS+KAPYEOUNPIEUP": 55284, "YESIEUNG+MIEUM": 55285, "YESIEUNG+HIEUH": 55286, "CIEUC+PIEUP": 55287, "CIEUC+PIEUP+PIEUP": 55288, "CIEUC+CIEUC": 55289, "PHIEUPH+SIOS": 55290, "PHIEUPH+THIEUTH": 55291}};
const JAMO_PRE_LIST = {"L": ["CEONGCHIEUMCHIEUCH", "CEONGCHIEUMCIEUC", "CEONGCHIEUMCIEUC+CEONGCHIEUMCIEUC", "CEONGCHIEUMSIOS", "CEONGCHIEUMSIOS+CEONGCHIEUMSIOS", "CHIEUCH", "CHIEUCH+HIEUH", "CHIEUCH+KHIEUKH", "CHITUEUMCHIEUCH", "CHITUEUMCIEUC", "CHITUEUMCIEUC+CHITUEUMCIEUC", "CHITUEUMSIOS", "CHITUEUMSIOS+CHITUEUMSIOS", "CIEUC", "CIEUC+CIEUC", "CIEUC+CIEUC+HIEUH", "CIEUC+IEUNG", "HIEUH", "HIEUH+HIEUH", "HIEUH+SIOS", "IEUNG", "IEUNG+CHIEUCH", "IEUNG+CIEUC", "IEUNG+HIEUH", "IEUNG+IEUNG", "IEUNG+KIYEOK", "IEUNG+MIEUM", "IEUNG+PANSIOS", "IEUNG+PHIEUPH", "IEUNG+PIEUP", "IEUNG+RIEUL", "IEUNG+SIOS", "IEUNG+THIEUTH", "IEUNG+TIKEUT", "KAPYEOUNMIEUM", "KAPYEOUNPHIEUPH", "KAPYEOUNPIEUP", "KAPYEOUNRIEUL", "KAPYEOUNSSANGPIEUP", "KHIEUKH", "KIYEOK", "KIYEOK+KIYEOK", "KIYEOK+TIKEUT", "MIEUM", "MIEUM+KIYEOK", "MIEUM+PIEUP", "MIEUM+SIOS", "MIEUM+TIKEUT", "NIEUN", "NIEUN+CIEUC", "NIEUN+HIEUH", "NIEUN+KIYEOK", "NIEUN+NIEUN", "NIEUN+PIEUP", "NIEUN+SIOS", "NIEUN+TIKEUT", "PANSIOS", "PHIEUPH", "PHIEUPH+HIEUH", "PHIEUPH+PIEUP", "PIEUP", "PIEUP+CHIEUCH", "PIEUP+CIEUC", "PIEUP+HIEUH", "PIEUP+KHIEUKH", "PIEUP+KIYEOK", "PIEUP+NIEUN", "PIEUP+PHIEUPH", "PIEUP+PIEUP", "PIEUP+SIOS", "PIEUP+SIOS+CIEUC", "PIEUP+SIOS+KIYEOK", "PIEUP+SIOS+PIEUP", "PIEUP+SIOS+SIOS", "PIEUP+SIOS+THIEUTH", "PIEUP+SIOS+TIKEUT", "PIEUP+THIEUTH", "PIEUP+TIKEUT", "RIEUL", "RIEUL+CIEUC", "RIEUL+HIEUH", "RIEUL+KAPYEOUNPIEUP", "RIEUL+KHIEUKH", "RIEUL+KIYEOK", "RIEUL+KIYEOK+KIYEOK", "RIEUL+MIEUM", "RIEUL+NIEUN", "RIEUL+PIEUP", "RIEUL+PIEUP+PIEUP", "RIEUL+RIEUL", "RIEUL+SIOS", "RIEUL+TIKEUT", "RIEUL+TIKEUT+TIKEUT", "SIOS", "SIOS+CHIEUCH", "SIOS+CIEUC", "SIOS+HIEUH", "SIOS+IEUNG", "SIOS+KHIEUKH", "SIOS+KIYEOK", "SIOS+MIEUM", "SIOS+NIEUN", "SIOS+PHIEUPH", "SIOS+PIEUP", "SIOS+PIEUP+KIYEOK", "SIOS+RIEUL", "SIOS+SIOS", "SIOS+SIOS+PIEUP", "SIOS+SIOS+SIOS", "SIOS+THIEUTH", "SIOS+TIKEUT", "THIEUTH", "THIEUTH+THIEUTH", "TIKEUT", "TIKEUT+CIEUC", "TIKEUT+KIYEOK", "TIKEUT+MIEUM", "TIKEUT+PIEUP", "TIKEUT+RIEUL", "TIKEUT+SIOS", "TIKEUT+TIKEUT", "YEORINHIEUH", "YEORINHIEUH+YEORINHIEUH", "YESIEUNG"], "V": ["A", "A+EU", "A+O", "A+U", "AE", "ARAEA", "ARAEA+A", "ARAEA+ARAEA", "ARAEA+E", "ARAEA+EO", "ARAEA+I", "ARAEA+U", "E", "EO", "EO+EU", "EO+O", "EO+U", "EU", "EU+A", "EU+E", "EU+EO", "EU+EU", "EU+I", "EU+I+U", "EU+O", "EU+U", "I", "I+A", "I+ARAEA", "I+EU", "I+I", "I+O", "I+O+I", "I+U", "I+YA", "I+YA+O", "I+YAE", "I+YE", "I+YEO", "I+YO", "I+YU", "O", "O+A", "O+AE", "O+E", "O+EO", "O+I", "O+O", "O+O+I", "O+U", "O+YA", "O+YAE", "O+YE", "O+YEO", "U", "U+A", "U+AE", "U+E", "U+EO", "U+EO+EU", "U+I", "U+I+I", "U+U", "U+YE", "U+YEO", "YA", "YA+O", "YA+U", "YA+YO", "YAE", "YE", "YEO", "YEO+O", "YEO+U", "YEO+YA", "YO", "YO+A", "YO+AE", "YO+EO", "YO+I", "YO+O", "YO+YA", "YO+YAE", "YO+YEO", "YU", "YU+A", "YU+AE", "YU+E", "YU+EO", "YU+I", "YU+O", "YU+U", "YU+YE", "YU+YEO"], "T": ["CHIEUCH", "CIEUC", "CIEUC+CIEUC", "CIEUC+PIEUP", "CIEUC+PIEUP+PIEUP", "HIEUH", "HIEUH+MIEUM", "HIEUH+NIEUN", "HIEUH+PIEUP", "HIEUH+RIEUL", "IEUNG", "IEUNG+IEUNG", "IEUNG+KHIEUKH", "IEUNG+KIYEOK", "IEUNG+KIYEOK+KIYEOK", "KAPYEOUNMIEUM", "KAPYEOUNPHIEUPH", "KAPYEOUNPIEUP", "KAPYEOUNRIEUL", "KHIEUKH", "KIYEOK", "KIYEOK+CHIEUCH", "KIYEOK+HIEUH", "KIYEOK+KHIEUKH", "KIYEOK+KIYEOK", "KIYEOK+NIEUN", "KIYEOK+PIEUP", "KIYEOK+RIEUL", "KIYEOK+SIOS", "KIYEOK+SIOS+KIYEOK", "MIEUM", "MIEUM+CHIEUCH", "MIEUM+CIEUC", "MIEUM+HIEUH", "MIEUM+KIYEOK", "MIEUM+MIEUM", "MIEUM+NIEUN", "MIEUM+NIEUN+NIEUN", "MIEUM+PANSIOS", "MIEUM+PIEUP", "MIEUM+PIEUP+SIOS", "MIEUM+RIEUL", "MIEUM+SIOS", "MIEUM+SIOS+SIOS", "NIEUN", "NIEUN+CHIEUCH", "NIEUN+CIEUC", "NIEUN+HIEUH", "NIEUN+KIYEOK", "NIEUN+NIEUN", "NIEUN+PANSIOS", "NIEUN+RIEUL", "NIEUN+SIOS", "NIEUN+THIEUTH", "NIEUN+TIKEUT", "PANSIOS", "PANSIOS+KAPYEOUNPIEUP", "PANSIOS+PIEUP", "PHIEUPH", "PHIEUPH+PIEUP", "PHIEUPH+SIOS", "PHIEUPH+THIEUTH", "PIEUP", "PIEUP+CHIEUCH", "PIEUP+CIEUC", "PIEUP+HIEUH", "PIEUP+MIEUM", "PIEUP+PHIEUPH", "PIEUP+PIEUP", "PIEUP+RIEUL", "PIEUP+RIEUL+PHIEUPH", "PIEUP+SIOS", "PIEUP+SIOS+TIKEUT", "PIEUP+TIKEUT", "RIEUL", "RIEUL+HIEUH", "RIEUL+KAPYEOUNPIEUP", "RIEUL+KHIEUKH", "RIEUL+KIYEOK", "RIEUL+KIYEOK+HIEUH", "RIEUL+KIYEOK+KIYEOK", "RIEUL+KIYEOK+SIOS", "RIEUL+MIEUM", "RIEUL+MIEUM+HIEUH", "RIEUL+MIEUM+KIYEOK", "RIEUL+MIEUM+SIOS", "RIEUL+NIEUN", "RIEUL+PANSIOS", "RIEUL+PHIEUPH", "RIEUL+PIEUP", "RIEUL+PIEUP+HIEUH", "RIEUL+PIEUP+PHIEUPH", "RIEUL+PIEUP+SIOS", "RIEUL+PIEUP+TIKEUT", "RIEUL+RIEUL", "RIEUL+RIEUL+KHIEUKH", "RIEUL+SIOS", "RIEUL+SIOS+SIOS", "RIEUL+THIEUTH", "RIEUL+TIKEUT", "RIEUL+TIKEUT+HIEUH", "RIEUL+YEORINHIEUH", "RIEUL+YEORINHIEUH+HIEUH", "RIEUL+YESIEUNG", "SIOS", "SIOS+CHIEUCH", "SIOS+CIEUC", "SIOS+HIEUH", "SIOS+KAPYEOUNPIEUP", "SIOS+KIYEOK", "SIOS+MIEUM", "SIOS+PANSIOS", "SIOS+PIEUP", "SIOS+RIEUL", "SIOS+SIOS", "SIOS+SIOS+KIYEOK", "SIOS+SIOS+TIKEUT", "SIOS+THIEUTH", "SIOS+TIKEUT", "THIEUTH", "TIKEUT", "TIKEUT+CHIEUCH", "TIKEUT+CIEUC", "TIKEUT+KIYEOK", "TIKEUT+PIEUP", "TIKEUT+RIEUL", "TIKEUT+SIOS", "TIKEUT+SIOS+KIYEOK", "TIKEUT+THIEUTH", "TIKEUT+TIKEUT", "TIKEUT+TIKEUT+PIEUP", "YEORINHIEUH", "YESIEUNG", "YESIEUNG+HIEUH", "YESIEUNG+MIEUM", "YESIEUNG+PANSIOS", "YESIEUNG+SIOS"]};

const JAMO_PRE = {
  L: new Set(JAMO_PRE_LIST.L),
  V: new Set(JAMO_PRE_LIST.V),
  T: new Set(JAMO_PRE_LIST.T),
};

// 물리 키(KeyQ ~ KeyM) -> (기본, Shift) 자모 매핑 (한컴 두벌식 옛글 + HNI 호환)
const KEY_LAYOUT = {
  KeyQ: { base: ['L', 'PIEUP', 'ㅂ'], shift: ['LL', 'PIEUP', 'ㅃ'] },
  KeyW: { base: ['L', 'CIEUC', 'ㅈ'], shift: ['LL', 'CIEUC', 'ㅉ'] },
  KeyE: { base: ['L', 'TIKEUT', 'ㄷ'], shift: ['LL', 'TIKEUT', 'ㄸ'] },
  KeyR: { base: ['L', 'KIYEOK', 'ㄱ'], shift: ['LL', 'KIYEOK', 'ㄲ'] },
  KeyT: { base: ['L', 'SIOS', 'ㅅ'], shift: ['LL', 'SIOS', 'ㅆ'] },
  KeyY: { base: ['V', 'YO', 'ㅛ'], shift: ['TONE', 1, '방점1(〮)'] },
  KeyU: { base: ['V', 'YEO', 'ㅕ'], shift: ['TONE', 2, '방점2(〯)'] },
  KeyI: { base: ['V', 'YA', 'ㅑ'], shift: ['V', 'YA', 'ㅑ'] },
  KeyO: { base: ['V', 'AE', 'ㅐ'], shift: ['V', 'YAE', 'ㅒ'] },
  KeyP: { base: ['V', 'E', 'ㅔ'], shift: ['V', 'YE', 'ㅖ'] },

  KeyA: { base: ['L', 'MIEUM', 'ㅁ'], shift: ['L', 'CHITUEUMCIEUC', 'ᅎ'] },
  KeyS: { base: ['L', 'NIEUN', 'ㄴ'], shift: ['L', 'CEONGCHIEUMCIEUC', 'ᅐ'] },
  KeyD: { base: ['L', 'IEUNG', 'ㅇ'], shift: ['L', 'YESIEUNG', 'ㆁ'] },
  KeyF: { base: ['L', 'RIEUL', 'ㄹ'], shift: ['L', 'PANSIOS', 'ㅿ'] },
  KeyG: { base: ['L', 'HIEUH', 'ㅎ'], shift: ['L', 'YEORINHIEUH', 'ㆆ'] },
  KeyH: { base: ['V', 'O', 'ㅗ'], shift: ['V', 'O', 'ㅗ'] },
  KeyJ: { base: ['V', 'EO', 'ㅓ'], shift: ['V', 'EO', 'ㅓ'] },
  KeyK: { base: ['V', 'A', 'ㅏ'], shift: ['V', 'ARAEA', 'ㆍ'] },
  KeyL: { base: ['V', 'I', 'ㅣ'], shift: ['ALONE', 'I', 'ᅵ따로'] },

  KeyZ: { base: ['L', 'KHIEUKH', 'ㅋ'], shift: ['L', 'CHITUEUMSIOS', 'ᄼ'] },
  KeyX: { base: ['L', 'THIEUTH', 'ㅌ'], shift: ['L', 'CEONGCHIEUMSIOS', 'ᄾ'] },
  KeyC: { base: ['L', 'CHIEUCH', 'ㅊ'], shift: ['L', 'CHITUEUMCHIEUCH', 'ᅔ'] },
  KeyV: { base: ['L', 'PHIEUPH', 'ㅍ'], shift: ['L', 'CEONGCHIEUMCHIEUCH', 'ᅕ'] },
  KeyB: { base: ['V', 'YU', 'ㅠ'], shift: ['V', 'YU', 'ㅠ'] },
  KeyN: { base: ['V', 'U', 'ㅜ'], shift: ['V', 'U', 'ㅜ'] },
  KeyM: { base: ['V', 'EU', 'ㅡ'], shift: ['V', 'EU', 'ㅡ'] },
};

const YEONSEO_MAP = {
  'PIEUP': 'KAPYEOUNPIEUP',
  'MIEUM': 'KAPYEOUNMIEUM',
  'PHIEUPH': 'KAPYEOUNPHIEUPH',
  'RIEUL': 'KAPYEOUNRIEUL',
  'PIEUP+PIEUP': 'KAPYEOUNSSANGPIEUP',
};

const TONE_CP = { 1: 0x302E, 2: 0x302F };

function canExtend(pos, seq) {
  return JAMO_PRE[pos].has(seq.join('+'));
}

function isComplete(pos, seq) {
  return seq.length > 0 && (seq.join('+') in JAMO_REV[pos]);
}

function composeSyllableToText(syl) {
  if (!syl.L.length && !syl.V.length && !syl.T.length && !syl.tone) return '';
  const lCp = syl.L.length ? JAMO_REV.L[syl.L.join('+')] : null;
  const vCp = syl.V.length ? JAMO_REV.V[syl.V.join('+')] : null;
  const tCp = syl.T.length ? JAMO_REV.T[syl.T.join('+')] : null;

  // 만약 현대 한글 음절(초성 0x1100~0x1112, 중성 0x1161~0x1175, 종성 없음 또는 0x11A8~0x11C2)이고 방점이 없으면
  // 완성형(NFC)으로 조합하여 일반 텍스트와의 호환성도 유지하고, 옛한글 자모가 하나라도 포함되면 첫가끝(NFD)으로 출력
  const isModernL = lCp !== null && lCp >= 0x1100 && lCp <= 0x1112;
  const isModernV = vCp !== null && vCp >= 0x1161 && vCp <= 0x1175;
  const isModernT = !syl.T.length || (tCp !== null && tCp >= 0x11A8 && tCp <= 0x11C2);

  let s = '';
  if (isModernL && isModernV && isModernT && !syl.tone) {
    const lIdx = lCp - 0x1100;
    const vIdx = vCp - 0x1161;
    const tIdx = syl.T.length ? (tCp - 0x11A7) : 0;
    s = String.fromCodePoint(0xAC00 + (lIdx * 21 + vIdx) * 28 + tIdx);
    return s;
  }

  if (syl.L.length) {
    s += String.fromCodePoint(lCp || 0x115F);
  } else if (syl.V.length || syl.T.length) {
    s += String.fromCodePoint(0x115F);
  }
  if (syl.V.length) {
    s += String.fromCodePoint(vCp || 0x1160);
  } else if (s.length) {
    s += String.fromCodePoint(0x1160);
  }
  if (tCp) {
    s += String.fromCodePoint(tCp);
  }
  if (syl.tone && TONE_CP[syl.tone]) {
    s += String.fromCodePoint(TONE_CP[syl.tone]);
  }
  return s;
}

class HancomOldHangulAutomata {
  constructor() {
    this.reset();
  }

  reset() {
    this.committed = []; // 확정된 조각(문자열 또는 음절 객체)
    this.current = this._emptySyl();
    this.pendingSpace = null; // 'MOVE' | 'VOWEL' | null
    this.pendingYeonseo = false;
    this.standaloneNextVowel = false;
  }

  _emptySyl() {
    return { L: [], V: [], T: [], TChunks: [], tone: 0 };
  }

  _isEmpty(syl) {
    return !syl.L.length && !syl.V.length && !syl.T.length && !syl.tone;
  }

  _flushCurrent() {
    if (!this._isEmpty(this.current)) {
      this.committed.push(this.current);
    }
    this.current = this._emptySyl();
  }

  _resolveYeonseo(isVowelNext) {
    if (!this.pendingYeonseo) return;
    this.pendingYeonseo = false;
    const c = this.current;
    const lastT = c.T.length ? c.T[c.T.length - 1] : null;
    if (isVowelNext || !lastT || !(lastT in YEONSEO_MAP)) {
      this._flushCurrent();
      this.current = this._emptySyl();
      this.current.L = ['IEUNG'];
    } else {
      c.T[c.T.length - 1] = YEONSEO_MAP[lastT];
    }
  }

  _moveTToNextL() {
    const c = this.current;
    const moved = c.T.slice();
    c.T = [];
    c.TChunks = [];
    if (!this._isEmpty(c)) {
      this.committed.push(c);
    }
    this.current = this._emptySyl();
    this.current.L = moved;
  }

  _resolveSpace(isVowelNext) {
    if (!this.pendingSpace) return;
    const mode = this.pendingSpace;
    this.pendingSpace = null;
    if (isVowelNext && mode === 'VOWEL') {
      this._flushCurrent();
      this.standaloneNextVowel = true;
    } else if (isVowelNext && mode === 'MOVE') {
      this._moveTToNextL();
    } else {
      this._flushCurrent();
      this.committed.push(' ');
    }
  }

  feedConsonant(name) {
    this._resolveYeonseo(false);
    this._resolveSpace(false);
    const c = this.current;

    // 받침 뒤 ㅇ -> 연서 대기
    if (
      name === 'IEUNG' &&
      c.V.length &&
      c.T.length &&
      c.T[c.T.length - 1] in YEONSEO_MAP &&
      c.TChunks[c.TChunks.length - 1] === 1
    ) {
      this.pendingYeonseo = true;
      return;
    }

    if (c.V.length) {
      if (c.T.length < 3 && canExtend('T', [...c.T, name])) {
        c.T.push(name);
        c.TChunks.push(1);
        return;
      }
      this._flushCurrent();
      this.current.L = [name];
      return;
    }

    // 초성 연서 (ㅂ+ㅇ -> ᄫ, ㅁ+ㅇ -> ᄝ, ㅍ+ㅇ -> ᅗ, ㄹ+ㅇ -> ᄛ, ㅃ+ㅇ -> ᄬ)
    if (name === 'IEUNG' && !c.V.length && !c.T.length && c.L.length) {
      if (c.L.length >= 2) {
        const k2 = c.L.slice(-2).join('+');
        if (k2 in YEONSEO_MAP) {
          c.L.splice(c.L.length - 2, 2, YEONSEO_MAP[k2]);
          return;
        }
      }
      const k1 = c.L[c.L.length - 1];
      if (k1 in YEONSEO_MAP) {
        c.L[c.L.length - 1] = YEONSEO_MAP[k1];
        return;
      }
    }

    if (c.L.length < 3 && canExtend('L', [...c.L, name])) {
      c.L.push(name);
      return;
    }

    this._flushCurrent();
    this.current.L = [name];
  }

  feedDoubleConsonant(name) {
    this._resolveYeonseo(false);
    this._resolveSpace(false);
    const pair = [name, name];
    const c = this.current;
    if (c.V.length) {
      if (c.T.length + 2 <= 3 && canExtend('T', [...c.T, ...pair])) {
        c.T.push(...pair);
        c.TChunks.push(2);
        return;
      }
      this._flushCurrent();
      this.current.L = pair.slice();
      return;
    }
    if (c.L.length + 2 <= 3 && canExtend('L', [...c.L, ...pair])) {
      c.L.push(...pair);
      return;
    }
    this._flushCurrent();
    this.current.L = pair.slice();
  }

  feedVowel(name) {
    this._resolveYeonseo(true);
    this._resolveSpace(true);
    if (!canExtend('V', [name])) return;
    const c = this.current;

    if (this.standaloneNextVowel) {
      this.standaloneNextVowel = false;
      c.V = [name];
      this._flushCurrent();
      return;
    }

    // 종성이 있을 때 모음이 오면 도깨비불 현상 (종성 끝 덩어리가 다음 초성으로 이동)
    if (c.T.length) {
      let acc = 0;
      for (let i = c.TChunks.length - 1; i >= 0; i--) {
        acc += c.TChunks[i];
        const rem = c.T.slice(0, c.T.length - acc);
        const mov = c.T.slice(c.T.length - acc);
        if (isComplete('L', mov) && (!rem.length || isComplete('T', rem))) {
          c.T = rem;
          c.TChunks = c.TChunks.slice(0, i);
          this._flushCurrent();
          this.current.L = mov;
          this.current.V = [name];
          return;
        }
      }
      this._flushCurrent();
      this.current.V = [name];
      return;
    }

    // ★ 한컴 오피스(한글과컴퓨터) 두벌식 옛글 핵심 규칙:
    // 'ㅏ'(A) 상태에서 다시 'ㅏ'(A)를 치면 아래아 'ㆍ'(ARAEA)로 변환!
    // 'ㆍ'(ARAEA) 상태에서 다시 'ㅏ'(A)를 치면 쌍아래아 'ᆢ'(ARAEA+ARAEA)로 변환!
    if (c.V.length === 1 && c.V[0] === 'A' && name === 'A') {
      c.V = ['ARAEA'];
      return;
    }
    if (c.V.length === 1 && c.V[0] === 'ARAEA' && name === 'A') {
      c.V = ['ARAEA', 'ARAEA'];
      return;
    }

    if (c.V.length) {
      if (c.V.length < 3 && canExtend('V', [...c.V, name])) {
        c.V.push(name);
        return;
      }
      this._flushCurrent();
      this.current.V = [name];
      return;
    }

    c.V = [name];
  }

  feedSpace(shiftKey = false) {
    if (shiftKey) {
      // Shift+Space: 빈칸 없이 현재 음절 닫기
      this.standaloneNextVowel = false;
      this._resolveYeonseo(false);
      if (this.pendingSpace) {
        this.pendingSpace = null;
        this._flushCurrent();
        this.committed.push(' ');
      }
      this._flushCurrent();
      return;
    }
    this._resolveYeonseo(false);
    if (this.pendingSpace === 'MOVE') {
      this.pendingSpace = null;
      this._moveTToNextL();
      return;
    }
    if (this.pendingSpace === 'VOWEL') {
      this._resolveSpace(false);
    }
    const c = this.current;
    if (c.T.length >= 1 && c.T.length <= 2 && isComplete('L', c.T)) {
      this.pendingSpace = 'MOVE';
      return;
    }
    if (c.V.length && !c.T.length) {
      this.pendingSpace = 'VOWEL';
      return;
    }
    this._flushCurrent();
    this.committed.push(' ');
  }

  feedStandaloneVowel(name = 'I') {
    this._resolveYeonseo(false);
    this._resolveSpace(false);
    this._flushCurrent();
    this.current.V = [name];
    this._flushCurrent();
  }

  feedTone(level) {
    this._resolveYeonseo(false);
    this._resolveSpace(false);
    let target = !this._isEmpty(this.current) ? this.current : null;
    if (!target) {
      for (let i = this.committed.length - 1; i >= 0; i--) {
        if (typeof this.committed[i] === 'object') {
          target = this.committed[i];
          break;
        }
      }
    }
    if (target) {
      target.tone = target.tone === level ? 0 : level;
    }
  }

  feedRawText(str) {
    this._resolveYeonseo(false);
    this._resolveSpace(false);
    this._flushCurrent();
    for (const ch of str) {
      this.committed.push(ch);
    }
  }

  backspace() {
    this.standaloneNextVowel = false;
    if (this.pendingYeonseo) {
      this.pendingYeonseo = false;
      return;
    }
    if (this.pendingSpace) {
      this.pendingSpace = null;
      return;
    }
    const c = this.current;
    if (!this._isEmpty(c)) {
      if (c.tone) {
        c.tone = 0;
        return;
      }
      if (c.T.length) {
        const n = c.TChunks.pop() || 1;
        c.T.splice(c.T.length - n, n);
        return;
      }
      if (c.V.length) {
        c.V.pop();
        return;
      }
      if (c.L.length) {
        c.L.pop();
        return;
      }
    }
    if (this.committed.length) {
      const last = this.committed.pop();
      if (typeof last === 'object') {
        this.current = last;
      }
    }
  }

  getText() {
    const parts = this.committed.slice();
    if (!this._isEmpty(this.current)) {
      parts.push(this.current);
    }
    if (this.pendingYeonseo) {
      const preview = this._emptySyl();
      preview.L = ['IEUNG'];
      parts.push(preview);
    }
    let out = '';
    for (const p of parts) {
      if (typeof p === 'string') {
        out += p;
      } else {
        out += composeSyllableToText(p);
      }
    }
    if (this.pendingSpace) {
      out += ' ';
    }
    return out;
  }

  getCurrentStateSummary() {
    const c = this.current;
    if (this._isEmpty(c) && !this.pendingYeonseo && !this.pendingSpace) {
      return '대기 중 (자모 타건 시 실시간 옛한글·현대한글 조합)';
    }
    const lStr = c.L.join('+') || '-';
    const vStr = c.V.join('+') || '-';
    const tStr = c.T.join('+') || '-';
    return `조합 중: 초성[${lStr}] · 중성[${vStr}] · 종성[${tStr}]`;
  }
}

window.HancomOldHangulAutomata = HancomOldHangulAutomata;
window.KEY_LAYOUT = KEY_LAYOUT;
