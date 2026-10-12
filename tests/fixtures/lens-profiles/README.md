# Lensfun 프로파일 검사 자료

`synthetic.xml`은 이 프로젝트에서 만든 좌표/모델/촬영 정보/잘못된 값 검사 자료다. 실제 렌즈의 화질 검증 자료가 아니다.

`slr-nikon.xml`은 [Lensfun 공식 데이터](https://github.com/lensfun/lensfun/blob/bbd4332a9ec566fd9aa548c9e0d8ced238c56261/data/db/slr-nikon.xml)의 커밋 `bbd4332a9ec566fd9aa548c9e0d8ced238c56261`에서 가져온 변경 없는 검사 자료다. SHA-256: `5e591e8b1032203cce4c822f1da3d49524a78e7d71c594ea5a4533f2e57aacdf`. 기여자/개별 교정 자료의 저작자 표시는 XML 안의 원래 주석에 보존한다. 데이터의 [CC BY-SA 3.0 라이선스](https://github.com/lensfun/lensfun/blob/bbd4332a9ec566fd9aa548c9e0d8ced238c56261/data/COPYING.CC_BY-SA_3.0)는 `COPYING.CC_BY-SA_3.0`에 포함했다. 실행 프로그램에 이 데이터베이스를 기본으로 배포하지 않는다.

왜곡 수식은 [공식 모델 설명](https://lensfun.github.io/manual/latest/group__Lens.html)과 [좌표 설명](https://lensfun.github.io/manual/latest/corrections.html)을 기준으로 Rust에서 구현했다. 네이티브 Lensfun 라이브러리 전체와 같은 결과 또는 실제 카메라의 보정 화질을 보장하는 검사가 아니다. 색수차3모델은 순차59에서 연결했으며 주변부 밝기2모델은 순차60에서 연결했으며 왜곡/색수차 초점거리 보간은 순차61에서 연결했으며 주변부 밝기의 초점거리/조리개/거리 보간은 순차62에서 연결했다. 실제 카메라/확장 매칭/색 관리 검증은 남는다. 자체 프로젝트에 선택된 렌즈의 원래 XML을 유지하며 미지원 요소도 지우지 않는다.

`verify_lens_profile_outputs.py`는 별도 NumPy 수식/float 샘플링으로 좌표·픽셀·분수 범위·RAW 원본·PSD/PSB 레이어를 확인한다.

`tca.xml`은 자체 생성한3종 색수차·왜곡 순서/색수차 전용·중복/잘못된 값 자료다. `verify_lens_tca_outputs.py`는 별도 수식으로72경우/8424float픽셀과 녹색/알파 기준 위치·RAW 범위·32비트 PSD/PSB를 비교한다. 공식 데이터와 실제 카메라 외관 검증을 구분한다.

`vignetting.xml`은 자체 생성 PA/ACM·조리개/교정 거리/중복·잘못된 분모·전용 값 자료다. `verify_lens_vignetting_outputs.py`는 별도 NumPy 선형 sRGB/반대각선·초점거리 단위/왜곡·색수차 적용 순서와90경우/10530픽셀, RAW 범위/자산/32비트 PSD·PSB를 확인한다. 공유 계산기는 `lens_profile_math.py`, `lens_tca_math.py`이며 제품 Rust 코드를 실행하지 않는다.

`interpolation.xml`은 자체 생성4왜곡/3색수차·4초점거리·두 교정 그룹/외부 이웃 검사 자료다. `verify_lens_interpolation_outputs.py`는 별도 XML 읽기와4×4연립방정식으로 cubic 경계 조건/계수 스케일/실제 초점거리를 비교하고 RAW 범위/원본/PSD·PSB를 확인한다. 기준 보간은 [고정 커밋 공식 설명/구현](https://github.com/lensfun/lensfun/blob/bbd4332a9ec566fd9aa548c9e0d8ced238c56261/libs/lensfun/lens.cpp)을 참고하며 전체 네이티브 Lensfun 결과 일치를 주장하지 않는다.

`vignette-interpolation.xml`은 자체 PA/ACM 줌·초점거리/조리개/거리/단일 초점거리·다중 교정 그룹 자료다. `verify_lens_vignette_interpolation_outputs.py`는 별도 XML과 NumPy 거리 가중 계산(역수 거리 지수3.5, 조리개/거리 역수 축, ACM 계수 스케일)으로21조건 및 RAW 범위/자산/32비트 PSD·PSB를 확인한다. 실제 카메라 화질을 주장하지 않는다.

순차63의 목록 검사는 기존 자체 XML들을 함께 불러오기/재수입/뒤 파일 실패/교정 그룹 충돌/검색/예산과 headless 선택·비우기에 사용한다. `qa-output/lens-catalog/summary.json`에 병합 개수와 구분 표시를 기록한다.

순차64는 자체 XML을 한글 경로에 복사해 별도 작업/설정 저장/재실행/누락·실패/재시도/취소·늦은 결과/목록 비우기와 headless 버튼을 검사한다. `qa-output/lens-library`의 설정·검사 파일을 사용하며 사용자 데이터 폴더나 실제 창에 접근하지 않는다.

순차65의 자체 중심/접선 교정576경우는 `qa-output/lens-orientation`에 기록한다. `verify_lens_orientation_outputs.py`가 독립 NumPy 광학 수식과 EXIF 픽셀 순열로8방향/107712픽셀, RAW 자산/저장 방향을 확인한다. 실제 카메라의 특수 센서 좌표나 전체 ICC 관리 검사는 별도다.
