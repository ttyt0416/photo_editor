# 로컬 의존성 수정

`Cargo.toml`의 `[patch.crates-io]`가 아래 패키지를 사용한다. 원본 소스와 라이선스 고지는 각 디렉터리에 보존한다.

| 패키지 | 원본 버전 | 수정 |
| --- | --- | --- |
| `avif-decode` | 3.0.0 | x86 `rav1d` 의존성에서 기본 asm 기능을 끄고 `bitdepth_8`, `bitdepth_16`을 켜 NASM 없이 Rust 경로로 빌드 |
| `openjp2` | 0.6.1 | `src/tcd.rs`의 타일 버퍼 초기화/해제 두 곳에서 null 포인터를 `std::alloc::dealloc`에 넘기지 않도록 검사. `src/types.rs`의 `ICCProfileRef<'_>` 수명 표기도 명시. 순차90은 `Stream::new_writer` 안전한 Rust 소유 스트림 생성자를 추가해 원자적 임시 파일에 JPEG2000을 출력 |

| `usvg` | 0.47.0 | `use`/symbol/중첩 SVG 변형의 절대 좌표 중복 적용 수정. 원래 SVG/CSS/참조 구조를 유지하며 상대 렌더링과 절대 편집 좌표를 일치시킨다. 자세한 변경과 출처는 [usvg 안내](usvg/LOCAL_CHANGES.md) |

openjp2 수정 전 독립 JP2 예제 디코딩이 Windows에서 프로세스를 중단했고, 수정 후 색·알파 확인 테스트가 통과했다. 새 upstream 버전으로 교체할 때 동일 테스트를 유지한다.

이 문서는 전체 배포 의존성의 라이선스 고지를 대신하지 않는다. 앱 패키징 시 전체 의존성의 조건과 고지를 함께 확인한다.

| `jpeg-encoder` | 0.7.1 | 최적화 Huffman 빈도 계산에서 재시작 경계의 DC 예측값을 초기화해 실제 쓰기 경로와 일치시킨다. [변경 안내](jpeg-encoder/LOCAL_CHANGES.md) |

| `zune-jpeg` | 0.5.15 | 여러 스캔 JPEG의 DC/재시작 초기화와 성분별 블록 행 좌표를 수정. [변경 안내](zune-jpeg/LOCAL_CHANGES.md) |

| `tiff` | 0.11.3 | IEEE 부동소수점 Predictor3의 스트립 쓰기 및 LZW/Deflate 예측 호환성 검증. 원본 MIT 고지를 보존. [변경 안내](tiff/LOCAL_CHANGES.md) |

순차91은 openjp2의 RPCL 내부 순서 변환 누락을 보완한다. 기존 패킷 반복과 연결하고5순서/전체·부분 품질을 독립 C 입력/출력으로 검사했다. [변경 기록](openjp2/LOCAL_CHANGES.md).

순차94는 openjp2의 안전한 JP2 출력 해상도 설정과 res /resd 헤더 쓰기를 추가한다. C 매개변수/이미지 ABI와 기존 라이선스는 유지하며 None/기본값은 원래 출력을 유지한다. [변경 기록](openjp2/LOCAL_CHANGES.md).

순차115는 openjp2의 ICC COLR 원문 복사·RGB/GRAY 알파 CDEF와 추가 ICC 헤더의 압축 예산을 보강한다. public C ABI/원래 고지는 유지한다. 포함/제외의 codestream/성분·독립 C 읽기·타일/품질층 회귀를 검사했다. [변경 기록](openjp2/LOCAL_CHANGES.md).
